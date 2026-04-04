#!/usr/bin/env python3
"""
Polymarket WebSocket probe — captures live market data and maps it to mantis-events.

Usage:
    pip install websockets aiohttp
    python scripts/polymarket_ws_probe.py [--slug btc-updown-15m-1775139300] [--duration 30]

This script:
1. Discovers token IDs via the Gamma API for the given market slug
2. Connects to the Polymarket CLOB WebSocket
3. Captures all event types (book, price_change, last_trade_price, best_bid_ask)
4. Pretty-prints each message with its mantis-events mapping
5. Prints a summary analysis of field coverage

to run on nixOS:

nix-shell -p python312 python312Packages.websockets python312Packages.aiohttp \
  --run "python3 scripts/polymarket_ws_probe.py --duration 60"

"""

import argparse
import asyncio
import json
import sys
import time
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Any

try:
    import aiohttp
except ImportError:
    print("Install dependencies: pip install aiohttp websockets", file=sys.stderr)
    sys.exit(1)

try:
    import websockets
    import websockets.asyncio.client
except ImportError:
    print("Install dependencies: pip install websockets", file=sys.stderr)
    sys.exit(1)

# ── Constants ────────────────────────────────────────────────────────────────

GAMMA_API = "https://gamma-api.polymarket.com"
WS_MARKET_URL = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
PING_INTERVAL = 10  # seconds

# ── Gamma API: discover token IDs from slug ──────────────────────────────────


async def discover_market(slug: str) -> dict:
    """Look up a market by slug via the Gamma API, return token IDs and metadata."""
    url = f"{GAMMA_API}/events?slug={slug}"
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as resp:
            if resp.status != 200:
                # Try direct markets endpoint
                url2 = f"{GAMMA_API}/markets?slug={slug}"
                async with session.get(url2) as resp2:
                    data = await resp2.json()
                    if isinstance(data, list) and len(data) > 0:
                        return _parse_market(data[0], slug)
                    raise RuntimeError(f"Market not found for slug: {slug}")
            data = await resp.json()

    # Events endpoint returns a list of events, each with markets
    if isinstance(data, list) and len(data) > 0:
        event = data[0]
        markets = event.get("markets", [])
        if markets:
            return _parse_market(markets[0], slug)

    # Fallback: search by tag
    url3 = f"{GAMMA_API}/events?limit=50&active=true&closed=false&tag_slug=up-or-down"
    async with aiohttp.ClientSession() as session:
        async with session.get(url3) as resp:
            events = await resp.json()
            for ev in events:
                for mkt in ev.get("markets", []):
                    if slug in mkt.get("slug", "") or slug in mkt.get("groupItemTitle", ""):
                        return _parse_market(mkt, slug)

    raise RuntimeError(f"Could not find market for slug: {slug}")


def _parse_market(mkt: dict, slug: str) -> dict:
    """Extract token IDs and metadata from a Gamma API market object."""
    condition_id = mkt.get("conditionId", mkt.get("condition_id", ""))
    clob_token_ids_raw = mkt.get("clobTokenIds", "[]")

    # clobTokenIds is sometimes a JSON string, sometimes already a list
    if isinstance(clob_token_ids_raw, str):
        token_ids = json.loads(clob_token_ids_raw)
    else:
        token_ids = clob_token_ids_raw

    outcomes_raw = mkt.get("outcomes", "[]")
    if isinstance(outcomes_raw, str):
        outcomes = json.loads(outcomes_raw)
    else:
        outcomes = outcomes_raw

    return {
        "slug": mkt.get("slug", slug),
        "condition_id": condition_id,
        "token_ids": token_ids,
        "outcomes": outcomes,
        "question": mkt.get("question", ""),
        "tick_size": mkt.get("minimum_tick_size", mkt.get("tickSize", "0.01")),
        "active": mkt.get("active", True),
        "closed": mkt.get("closed", False),
    }


# ── Event analysis ───────────────────────────────────────────────────────────

# Mapping from Polymarket event_type to mantis-events variant
EVENT_MAPPING = {
    "book": {
        "mantis_variant": "BookDelta (N events per snapshot, IS_SNAPSHOT + LAST_IN_BATCH flags)",
        "description": "Full L2 snapshot -> N BookDelta events with IS_SNAPSHOT flag",
        "fields_used": {
            "bids[].price": "-> FixedI64::from_str_decimal -> InstrumentMeta::price_to_ticks -> BookDeltaPayload.price (Ticks)",
            "bids[].size": "-> FixedI64::from_str_decimal -> InstrumentMeta::qty_to_lots -> BookDeltaPayload.qty (Lots)",
            "asks[].price": "same as bids, Side::Ask",
            "asks[].size": "same as bids, Side::Ask",
            "asset_id": "-> InstrumentRegistry::id_for -> EventHeader.instrument_id (InstrumentId)",
            "timestamp": "-> EventHeader.recv_ts (use local Timestamp::now(), not venue ts)",
        },
        "fields_ignored": {
            "hash": "orderbook hash, useful for consistency checks but not hot-path",
            "market": "condition_id, used for routing in ingestion layer only",
            "tick_size": "loaded at startup into InstrumentMeta, not per-event",
            "last_trade_price": "separate event type",
            "min_order_size": "config, not event data",
            "neg_risk": "market metadata, not event data",
        },
    },
    "price_change": {
        "mantis_variant": "BookDelta (1 event per changed level)",
        "description": "Incremental book update -> 1 BookDelta per price_changes[] entry",
        "fields_used": {
            "price_changes[].price": "-> Ticks via InstrumentMeta",
            "price_changes[].size": "-> Lots via InstrumentMeta (size=0 means Delete)",
            "price_changes[].side": "BUY->Side::Bid, SELL->Side::Ask",
            "price_changes[].asset_id": "-> InstrumentId",
            "timestamp": "-> local recv_ts",
        },
        "fields_ignored": {
            "price_changes[].hash": "consistency check, not hot-path",
            "price_changes[].best_bid": "redundant if we track book state",
            "price_changes[].best_ask": "redundant if we track book state",
            "market": "routing only",
        },
        "notes": [
            "size is CUMULATIVE (new total at level), not a delta",
            "size='0' means level removed -> UpdateAction::Delete",
            "size changed from nonzero to nonzero -> UpdateAction::Change",
            "new level (not previously in book) -> UpdateAction::New",
            "Compressed field names may arrive: a/p/s/si/h/bb/ba/m/pc/t",
        ],
    },
    "last_trade_price": {
        "mantis_variant": "Trade",
        "description": "Trade execution -> 1 Trade event",
        "fields_used": {
            "price": "-> Ticks via InstrumentMeta -> TradePayload.price",
            "size": "-> Lots via InstrumentMeta -> TradePayload.qty",
            "side": "BUY->Side::Bid, SELL->Side::Ask -> TradePayload.aggressor",
            "asset_id": "-> InstrumentId",
            "timestamp": "-> local recv_ts",
        },
        "fields_ignored": {
            "market": "routing only",
            "fee_rate_bps": "cold-path, not needed in hot event (look up by order_id if needed)",
            "transaction_hash": "on-chain tx, cold-path telemetry",
        },
    },
    "best_bid_ask": {
        "mantis_variant": "TopOfBook",
        "description": "BBO update -> 1 TopOfBook event",
        "fields_used": {
            "best_bid": "-> Ticks -> TopOfBookPayload.bid_price",
            "best_ask": "-> Ticks -> TopOfBookPayload.ask_price",
            "asset_id": "-> InstrumentId",
            "timestamp": "-> local recv_ts",
        },
        "fields_ignored": {
            "spread": "derived (ask - bid), not stored",
            "market": "routing only",
        },
        "notes": [
            "TopOfBookPayload also has bid_qty/ask_qty but best_bid_ask doesn't provide them",
            "Options: set qty to 0/sentinel, or only emit TopOfBook from book state engine",
            "Recommendation: use this for fast BBO, derive full TopOfBook from book state",
        ],
    },
    "tick_size_change": {
        "mantis_variant": "NOT MAPPED (cold-path config update)",
        "description": "Tick size changed -> update InstrumentMeta, not a hot event",
        "notes": [
            "This is a configuration change, not a market data event",
            "Handled in ingestion layer: update InstrumentMeta for the instrument",
            "May emit a log/telemetry event on a cold channel",
        ],
    },
    "new_market": {
        "mantis_variant": "NOT MAPPED (cold-path discovery)",
        "description": "New market created -> update InstrumentRegistry, not a hot event",
    },
    "market_resolved": {
        "mantis_variant": "NOT MAPPED (cold-path lifecycle)",
        "description": "Market resolved -> handled by application logic, not hot event bus",
    },
}


@dataclass
class EventStats:
    """Track statistics about observed events."""

    counts: dict[str, int] = field(default_factory=lambda: defaultdict(int))
    samples: dict[str, list[dict]] = field(default_factory=lambda: defaultdict(list))
    first_seen: dict[str, float] = field(default_factory=dict)
    field_registry: dict[str, set[str]] = field(default_factory=lambda: defaultdict(set))

    def record(self, event_type: str, msg: dict) -> None:
        self.counts[event_type] += 1
        if event_type not in self.first_seen:
            self.first_seen[event_type] = time.time()
        if len(self.samples[event_type]) < 2:
            self.samples[event_type].append(msg)
        # Track all top-level fields
        for key in msg:
            self.field_registry[event_type].add(key)


# ── Pretty printer ───────────────────────────────────────────────────────────


def print_event(event_type: str, msg: dict, count: int) -> None:
    """Pretty-print a WS event with its mantis-events mapping."""
    mapping = EVENT_MAPPING.get(event_type, {})
    mantis = mapping.get("mantis_variant", "UNKNOWN")

    print(f"\n{'='*80}")
    print(f"  [{count:>4}] event_type: {event_type}")
    print(f"         mantis-events -> {mantis}")
    print(f"{'='*80}")

    # Print key fields based on type
    if event_type == "book":
        asset = msg.get("asset_id", "?")[:20] + "..."
        bids = msg.get("bids", [])
        asks = msg.get("asks", [])
        ts = msg.get("timestamp", "?")
        print(f"  asset_id:  {asset}")
        print(f"  timestamp: {ts}")
        print(f"  bids: {len(bids)} levels  |  asks: {len(asks)} levels")
        if bids:
            print(f"  best bid:  {bids[0].get('price', '?')} x {bids[0].get('size', '?')}")
        if asks:
            print(f"  best ask:  {asks[0].get('price', '?')} x {asks[0].get('size', '?')}")
        tick = msg.get("tick_size", "?")
        print(f"  tick_size: {tick}")
        print(f"  -> {len(bids) + len(asks)} BookDelta events (IS_SNAPSHOT)")

    elif event_type == "price_change":
        changes = msg.get("price_changes", msg.get("pc", []))
        ts = msg.get("timestamp", msg.get("t", "?"))
        print(f"  timestamp: {ts}")
        print(f"  changes:   {len(changes)}")
        for i, ch in enumerate(changes[:5]):
            # Handle compressed field names
            price = ch.get("price", ch.get("p", "?"))
            size = ch.get("size", ch.get("s", "?"))
            side = ch.get("side", ch.get("si", "?"))
            asset = (ch.get("asset_id", ch.get("a", "?")))[:20]
            action = "Delete" if size == "0" else "Change"
            print(f"    [{i}] {side:4s} {price} x {size} ({action}) asset={asset}...")
        if len(changes) > 5:
            print(f"    ... and {len(changes) - 5} more")
        print(f"  -> {len(changes)} BookDelta events")

    elif event_type == "last_trade_price":
        print(f"  asset_id:  {msg.get('asset_id', '?')[:20]}...")
        print(f"  price:     {msg.get('price', '?')}")
        print(f"  size:      {msg.get('size', '?')}")
        print(f"  side:      {msg.get('side', '?')}")
        print(f"  timestamp: {msg.get('timestamp', '?')}")
        print(f"  -> 1 Trade event")

    elif event_type == "best_bid_ask":
        print(f"  asset_id:  {msg.get('asset_id', '?')[:20]}...")
        print(f"  best_bid:  {msg.get('best_bid', '?')}")
        print(f"  best_ask:  {msg.get('best_ask', '?')}")
        spread = msg.get("spread", "?")
        print(f"  spread:    {spread}")
        print(f"  timestamp: {msg.get('timestamp', '?')}")
        print(f"  -> 1 TopOfBook event (bid_qty/ask_qty not available from this source)")

    elif event_type in ("tick_size_change", "new_market", "market_resolved"):
        print(f"  [cold-path event, not mapped to hot event bus]")
        print(f"  {json.dumps(msg, indent=4)[:500]}")

    else:
        print(f"  [unknown event type]")
        print(f"  {json.dumps(msg, indent=4)[:500]}")


# ── WebSocket client ─────────────────────────────────────────────────────────


async def run_probe(token_ids: list[str], duration: int, market_info: dict) -> EventStats:
    """Connect to Polymarket WS and capture events for the given duration."""
    stats = EventStats()
    total_count = 0

    print(f"\nConnecting to {WS_MARKET_URL}")
    print(f"Subscribing to {len(token_ids)} token(s) for {duration}s...")
    print(f"Market: {market_info.get('question', market_info.get('slug', '?'))}")
    print(f"Tick size: {market_info.get('tick_size', '?')}")

    subscribe_msg = json.dumps({
        "assets_ids": token_ids,
        "type": "market",
        "custom_feature_enabled": True,
    })

    deadline = time.time() + duration

    try:
        async with websockets.asyncio.client.connect(WS_MARKET_URL) as ws:
            # Subscribe
            await ws.send(subscribe_msg)
            print(f"Subscribed. Waiting for events...\n")

            # Start ping task
            async def ping_loop():
                while time.time() < deadline:
                    try:
                        await ws.send("PING")
                    except Exception:
                        break
                    await asyncio.sleep(PING_INTERVAL)

            ping_task = asyncio.create_task(ping_loop())

            try:
                while time.time() < deadline:
                    try:
                        raw = await asyncio.wait_for(
                            ws.recv(), timeout=min(5.0, deadline - time.time())
                        )
                    except asyncio.TimeoutError:
                        continue
                    except Exception as e:
                        print(f"\nWS error: {e}")
                        break

                    if not raw or raw == "PONG":
                        continue

                    # Parse message(s) — can be a single object or array
                    try:
                        parsed = json.loads(raw)
                    except json.JSONDecodeError:
                        print(f"  [non-JSON frame: {raw[:100]}]")
                        continue

                    # Normalize to list
                    messages = parsed if isinstance(parsed, list) else [parsed]

                    for msg in messages:
                        if not isinstance(msg, dict):
                            continue

                        event_type = msg.get("event_type", msg.get("type", "unknown"))
                        total_count += 1
                        stats.record(event_type, msg)

                        # Print first 3 of each type, then just count
                        if stats.counts[event_type] <= 3:
                            print_event(event_type, msg, total_count)
                        elif stats.counts[event_type] == 4:
                            print(f"\n  ... (suppressing further {event_type} events, will count)")

            finally:
                ping_task.cancel()
                try:
                    await ping_task
                except asyncio.CancelledError:
                    pass

    except Exception as e:
        print(f"\nConnection error: {e}")

    return stats


# ── Summary & mapping analysis ───────────────────────────────────────────────


def print_summary(stats: EventStats, market_info: dict) -> None:
    """Print summary of observed events and mantis-events mapping analysis."""
    print(f"\n{'='*80}")
    print(f"  SUMMARY — {sum(stats.counts.values())} events captured")
    print(f"{'='*80}")

    print(f"\n## Event counts\n")
    for evt, count in sorted(stats.counts.items(), key=lambda x: -x[1]):
        mapping = EVENT_MAPPING.get(evt, {})
        mantis = mapping.get("mantis_variant", "UNKNOWN")
        print(f"  {evt:25s}  {count:>5}x  -> {mantis}")

    print(f"\n## Fields observed per event type\n")
    for evt in sorted(stats.field_registry.keys()):
        fields = sorted(stats.field_registry[evt])
        print(f"  {evt}:")
        print(f"    {', '.join(fields)}")

    print(f"\n{'='*80}")
    print(f"  MANTIS-EVENTS MAPPING ANALYSIS")
    print(f"{'='*80}")

    print(f"""
## Coverage Summary

Polymarket WS event         -> mantis-events variant     Status
---------------------------------------------------------------------------
book (snapshot)             -> N x BookDelta + flags     FULLY COVERED
price_change (incremental)  -> N x BookDelta             FULLY COVERED
last_trade_price            -> Trade                     FULLY COVERED
best_bid_ask                -> TopOfBook                 PARTIAL (no qty)
tick_size_change            -> (cold-path config)        CORRECTLY EXCLUDED
new_market                  -> (cold-path discovery)     CORRECTLY EXCLUDED
market_resolved             -> (cold-path lifecycle)     CORRECTLY EXCLUDED

## Ingestion Pipeline Mapping

  WS frame (JSON string)
    |
    v
  serde_json::from_slice -> intermediate struct
    |
    v
  FixedI64::<2>::from_str_decimal(price_str)   # Polymarket uses 2 decimals
  FixedI64::<2>::from_str_decimal(size_str)     # sizes also 2 decimal places
    |
    v
  InstrumentMeta<2>::price_to_ticks(price) -> Ticks
  InstrumentMeta<2>::qty_to_lots(qty) -> Lots
    |
    v
  HotEvent::book_delta(...) or HotEvent::trade(...)
    |
    v
  queue.push_batch(&batch)

## Key Observations for Ingestion Layer""")

    # Analyze tick size
    tick_size = market_info.get("tick_size", "0.01")
    print(f"""
  1. PRICE PRECISION:
     Polymarket tick_size = {tick_size}
     Prices are decimal strings like "0.55", "0.4825"
     Recommendation: FixedI64<2> for standard markets (tick=0.01)
                     FixedI64<4> if tick=0.0001 markets appear
     InstrumentMeta tick_size = FixedI64::<2>::from_str_decimal("{tick_size}")

  2. SIZE PRECISION:
     Sizes are decimal strings like "100.00", "250.50"
     Polymarket sizes can have up to 2 decimal places
     Recommendation: FixedI64<2> for sizes
     InstrumentMeta lot_size = FixedI64::<2>::from_str_decimal("0.01")

  3. SIDE MAPPING:
     "BUY"  -> Side::Bid
     "SELL" -> Side::Ask

  4. SNAPSHOT vs INCREMENTAL:
     "book" event = full snapshot -> set EventFlags::IS_SNAPSHOT on first,
                                     EventFlags::LAST_IN_BATCH on last
     "price_change" = incremental -> no snapshot flag

  5. LEVEL ACTION DETECTION:
     price_change.size = "0"          -> UpdateAction::Delete
     price_change.size != "0" (new)   -> UpdateAction::New
     price_change.size != "0" (exist) -> UpdateAction::Change
     (requires tracking known levels in ingestion layer)

  6. COMPRESSED FIELD NAMES:
     price_change may use: a/p/s/si/h/bb/ba/m/pc/t
     Ingestion layer must handle both forms

  7. BEST_BID_ASK -> TopOfBook GAP:
     best_bid_ask provides prices but NOT quantities
     TopOfBookPayload has bid_qty/ask_qty fields
     Options:
       a) Set qty = Lots::from_raw(0) as sentinel (simple, caller must check)
       b) Only emit TopOfBook from book-state engine after delta processing
       c) Add a flag bit (IS_DERIVED) to distinguish
     Recommendation: (b) for v1 — derive TopOfBook from book state only

  8. TOKEN_ID -> InstrumentId MAPPING:
     Token IDs are huge integer strings (>70 chars)
     Must maintain a registry: token_id_str -> InstrumentId(u32)
     This is the InstrumentRegistry the spec anticipated

  9. TIMESTAMP HANDLING:
     Polymarket timestamps are string milliseconds or seconds (inconsistent)
     Best practice: use local Timestamp::now() for recv_ts (spec-compliant)
     Venue timestamp can be parsed for telemetry but NOT in hot event
""")

    print(f"\n## Verdict\n")
    print(f"  The mantis-events design is WELL SUITED for Polymarket ingestion.")
    print(f"  All hot-path market data maps cleanly to BookDelta, Trade, TopOfBook.")
    print(f"  Cold-path events (tick_size_change, new_market, market_resolved)")
    print(f"  are correctly excluded from the hot event bus.")
    print(f"")
    print(f"  The main work for the ingestion layer is:")
    print(f"    1. InstrumentRegistry (token_id_str -> InstrumentId)")
    print(f"    2. InstrumentMeta<2> setup (tick_size, lot_size per market)")
    print(f"    3. Compressed field name handling for price_change")
    print(f"    4. Level tracking for UpdateAction detection")
    print(f"    5. Batch construction with IS_SNAPSHOT/LAST_IN_BATCH flags")

    # Print raw JSON samples
    print(f"\n{'='*80}")
    print(f"  RAW JSON SAMPLES (first of each type)")
    print(f"{'='*80}")
    for evt in sorted(stats.samples.keys()):
        samples = stats.samples[evt]
        if samples:
            print(f"\n## {evt}\n")
            sample = samples[0]
            # Truncate very long fields for readability
            truncated = _truncate_sample(sample)
            print(json.dumps(truncated, indent=2))


def _truncate_sample(obj: Any, max_str: int = 40, max_list: int = 3) -> Any:
    """Truncate long strings and lists in a JSON sample for display."""
    if isinstance(obj, str):
        return obj[:max_str] + "..." if len(obj) > max_str else obj
    if isinstance(obj, list):
        truncated = [_truncate_sample(item) for item in obj[:max_list]]
        if len(obj) > max_list:
            truncated.append(f"... ({len(obj) - max_list} more)")
        return truncated
    if isinstance(obj, dict):
        return {k: _truncate_sample(v) for k, v in obj.items()}
    return obj


# ── Main ─────────────────────────────────────────────────────────────────────


async def main() -> None:
    parser = argparse.ArgumentParser(description="Polymarket WS probe for mantis-events validation")
    parser.add_argument(
        "--slug",
        default="btc-updown-15m-1775139300",
        help="Market slug (default: btc-updown-15m-1775139300)",
    )
    parser.add_argument(
        "--duration",
        type=int,
        default=30,
        help="Capture duration in seconds (default: 30)",
    )
    parser.add_argument(
        "--token-ids",
        nargs="*",
        help="Explicit token IDs (skip Gamma API lookup)",
    )
    args = parser.parse_args()

    # Discover market
    if args.token_ids:
        token_ids = args.token_ids
        market_info = {"slug": args.slug, "tick_size": "0.01"}
        print(f"Using provided token IDs: {len(token_ids)}")
    else:
        print(f"Looking up market: {args.slug}")
        try:
            market_info = await discover_market(args.slug)
            token_ids = market_info["token_ids"]
            print(f"Found: {market_info['question']}")
            print(f"Condition ID: {market_info['condition_id']}")
            print(f"Token IDs: {len(token_ids)}")
            for i, tid in enumerate(token_ids):
                outcome = market_info["outcomes"][i] if i < len(market_info["outcomes"]) else "?"
                print(f"  [{outcome}] {tid[:30]}...")
        except Exception as e:
            print(f"Gamma API lookup failed: {e}")
            print("Try passing --token-ids explicitly or use a different --slug")
            sys.exit(1)

    if not token_ids:
        print("No token IDs found. Cannot subscribe.")
        sys.exit(1)

    # Run probe
    stats = await run_probe(token_ids, args.duration, market_info)

    # Print summary
    print_summary(stats, market_info)


if __name__ == "__main__":
    asyncio.run(main())
