#!/usr/bin/env python3
"""Analyze order types from fill patterns + on-chain data."""

import json
import requests
import time
from datetime import datetime, timezone, timedelta
from collections import defaultdict

API_URL = "https://narrative.agent.heisenberg.so/api/v2/semantic/retrieve/parameterized"
HEADERS = {
    "Authorization": "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ0b2tlbl90eXBlIjoiYWNjZXNzIiwiZXhwIjoxNzgwMzIyOTcyLCJpYXQiOjE3NzUxMzg5NzIsImp0aSI6ImNiMWJkZTk4Y2FlYTRhMTg5NmVjMzVkMGMxMWJlMjcyIiwidXNlcl9pZCI6MTI0OCwic2NvcGUiOiJsYXVuY2hwYWQ6YWdlbnQtcmVhZCxyZXRyaWV2ZXI6ZWNoby1nZW5lcmF0aW9uLHJldHJpZXZlcjpmZWF0dXJlLWV4dHJhY3Rpb24sdXNlcjpyZWFkLHJldHJpZXZlcjphZ2VudC1vcHRpb24tcmV0cmlldmFsLGxhdW5jaHBhZDphZ2VudC1jcmVhdGlvbixsYXVuY2hwYWQ6YWdlbnQtdXBkYXRlLHVzZXI6d3JpdGUscmV0cmlldmVyOnNlbWFudGljLXJldHJpZXZhbCxsYXVuY2hwYWQ6ZWNoby1zdHlsZS1jcmVhdGlvbiIsInRva2VuX25hbWUiOiJiYXNlX2xvZ2luIn0.L3O49KE6uFtqkXGaXQrEA6lmBKlnjzGatd9niJ3CRkM",
    "Content-Type": "application/json",
}
WALLET = "0xe1d6b51521bd4365769199f392f9818661bd907c"


def extract_results(result):
    if not result:
        return []
    if isinstance(result, dict):
        if "data" in result and isinstance(result["data"], dict):
            if "results" in result["data"]:
                return result["data"]["results"]
        if "data" in result and isinstance(result["data"], list):
            return result["data"]
        if "results" in result:
            return result["results"]
    return []


# Get trades from last hour for analysis
now = datetime.now(timezone.utc)
start = int((now - timedelta(hours=2)).timestamp())
end = int(now.timestamp())

print("Fetching recent trades...")
all_trades = []
offset = 0
for page in range(15):
    r = requests.post(API_URL, headers=HEADERS, json={
        "agent_id": 556,
        "params": {
            "proxy_wallet": WALLET,
            "condition_id": "ALL",
            "start_time": str(start),
            "end_time": str(end),
        },
        "pagination": {"limit": 200, "offset": offset},
        "formatter_config": {"format_type": "raw"},
    }, timeout=60)
    res = r.json()
    trades = extract_results(res)
    if not trades:
        break
    all_trades.extend(trades)
    if not res.get("pagination", {}).get("has_more", False):
        break
    offset += 200
    time.sleep(0.15)

btc_15m = [t for t in all_trades if "btc-updown-15m" in t.get("slug", "").lower()]
print(f"Total trades: {len(all_trades)} | BTC 15m: {len(btc_15m)}")

# ── Analysis 1: Fill Pattern Analysis ────────────────────────────────────────
print()
print("=" * 100)
print("FILL PATTERN ANALYSIS — Inferring Order Types")
print("=" * 100)

# Group by slug
by_slug = defaultdict(list)
for t in btc_15m:
    by_slug[t.get("slug", "")].append(t)

for slug in sorted(by_slug.keys()):
    trades = sorted(by_slug[slug], key=lambda x: x.get("timestamp", ""))
    if len(trades) < 10:
        continue

    print(f"\n--- {slug} ({len(trades)} trades) ---")

    # Group fills by SECOND (same timestamp = likely same order hitting book)
    by_second = defaultdict(list)
    for t in trades:
        ts_str = t.get("timestamp", "")[:19]  # truncate to second
        by_second[ts_str].append(t)

    # Analyze clusters
    print(f"\n  Fill clusters (trades in same second):")
    print(f"  {'Time':>10} | {'#Fills':>6} | {'Outcome':>7} | {'Prices':>30} | {'Sizes':>30} | {'TxHashes':>10}")
    print("  " + "-" * 105)

    multi_fill_clusters = 0
    single_fills = 0

    for ts_str in sorted(by_second.keys()):
        fills = by_second[ts_str]
        time_short = ts_str[11:]

        if len(fills) == 1:
            single_fills += 1
            t = fills[0]
            tx = t.get("transaction_hash", t.get("id", ""))[:16]
            print(f"  {time_short:>10} | {1:>6} | {t.get('outcome', ''):>7} | "
                  f"${float(t.get('price', 0)):.4f}{' ':>24} | "
                  f"{float(t.get('size', 0)):>8.1f}{' ':>21} | {tx}...")
        else:
            multi_fill_clusters += 1
            # Check if same tx hash or different
            tx_hashes = set()
            for t in fills:
                tx = t.get("transaction_hash", t.get("id", ""))
                if tx:
                    # The id field has format txhash_orderhash
                    parts = tx.split("_")
                    tx_hashes.add(parts[0][:16] if parts else tx[:16])

            prices = [float(t.get("price", 0)) for t in fills]
            sizes = [float(t.get("size", 0)) for t in fills]
            outcomes = set(t.get("outcome", "") for t in fills)
            outcome_str = "/".join(outcomes)

            unique_prices = len(set(f"{p:.4f}" for p in prices))
            total_size = sum(sizes)
            total_usdc = sum(s * p for s, p in zip(sizes, prices))

            price_range = f"${min(prices):.4f}-${max(prices):.4f}" if unique_prices > 1 else f"${prices[0]:.4f}"
            size_range = f"{min(sizes):.1f}-{max(sizes):.1f} (tot:{total_size:.0f})"

            # Multiple tx hashes in same second = multiple orders
            # Single tx hash with multiple fills = one order sweeping the book
            n_tx = len(tx_hashes)
            order_type_hint = ""
            if n_tx == 1 and unique_prices > 1:
                order_type_hint = "← MARKET/FOK (1 tx, multiple prices = book sweep)"
            elif n_tx == 1 and unique_prices == 1:
                order_type_hint = "← LIMIT fill (1 tx, 1 price)"
            elif n_tx > 1 and unique_prices == 1:
                order_type_hint = f"← {n_tx} SEPARATE ORDERS same price (GTC resting?)"
            elif n_tx > 1 and unique_prices > 1:
                order_type_hint = f"← {n_tx} SEPARATE ORDERS diff prices"

            print(f"  {time_short:>10} | {len(fills):>6} | {outcome_str:>7} | "
                  f"{price_range:>30} | {size_range:>30} | {n_tx} tx {order_type_hint}")

    print(f"\n  Summary: {multi_fill_clusters} multi-fill clusters, {single_fills} single fills")

    # ── Analysis 2: TX Hash Pattern ──────────────────────────────────────
    print(f"\n  TX Hash Analysis:")

    # The trade ID format is: txhash_orderhash
    # If same order fills multiple times, orderhash should be the same
    order_hashes = defaultdict(list)  # orderhash -> list of fills
    tx_to_fills = defaultdict(list)  # txhash -> list of fills

    for t in trades:
        tid = t.get("id", "")
        if "_" in tid:
            tx_hash, order_hash = tid.split("_", 1)
            order_hashes[order_hash].append(t)
            tx_to_fills[tx_hash].append(t)
        else:
            tx_hash = t.get("transaction_hash", tid)
            tx_to_fills[tx_hash].append(t)

    # Orders that got multiple fills
    multi_fill_orders = {oh: fills for oh, fills in order_hashes.items() if len(fills) > 1}

    print(f"  Unique order hashes: {len(order_hashes)}")
    print(f"  Unique tx hashes: {len(tx_to_fills)}")
    print(f"  Orders with multiple fills: {len(multi_fill_orders)}")

    if multi_fill_orders:
        print(f"\n  Multi-fill orders (same order_hash, multiple fills):")
        for oh, fills in sorted(multi_fill_orders.items(), key=lambda x: -len(x[1]))[:5]:
            prices = [float(f.get("price", 0)) for f in fills]
            sizes = [float(f.get("size", 0)) for f in fills]
            total = sum(s * p for s, p in zip(sizes, prices))
            unique_p = len(set(f"{p:.4f}" for p in prices))
            outcome = fills[0].get("outcome", "")
            print(f"    order {oh[:20]}... | {len(fills)} fills | {outcome} | "
                  f"prices: {unique_p} unique (${min(prices):.4f}-${max(prices):.4f}) | "
                  f"total: {sum(sizes):.0f} shares ${total:.0f}")

    # Tx with multiple fills (different orders in same tx = batch?)
    multi_fill_tx = {tx: fills for tx, fills in tx_to_fills.items() if len(fills) > 1}
    if multi_fill_tx:
        print(f"\n  Transactions with multiple fills (same tx, different orders):")
        for tx, fills in sorted(multi_fill_tx.items(), key=lambda x: -len(x[1]))[:5]:
            outcomes = set(f.get("outcome", "") for f in fills)
            prices = [float(f.get("price", 0)) for f in fills]
            sizes = [float(f.get("size", 0)) for f in fills]
            # Check if same or different order_hashes
            ohs = set()
            for f in fills:
                tid = f.get("id", "")
                if "_" in tid:
                    ohs.add(tid.split("_", 1)[1][:16])
            print(f"    tx {tx[:20]}... | {len(fills)} fills | outcomes: {outcomes} | "
                  f"orders: {len(ohs)} unique | "
                  f"prices ${min(prices):.4f}-${max(prices):.4f}")

    # ── Analysis 3: Price Walking Pattern ────────────────────────────────
    print(f"\n  Price Walking Analysis (consecutive fills):")

    # For each outcome separately, check if prices increase (walking the book up)
    for outcome in ["Up", "Down"]:
        outcome_trades = [t for t in trades if t.get("outcome") == outcome]
        if len(outcome_trades) < 3:
            continue

        price_changes = []
        for i in range(1, len(outcome_trades)):
            p_prev = float(outcome_trades[i - 1].get("price", 0))
            p_curr = float(outcome_trades[i].get("price", 0))
            if p_prev > 0 and p_curr > 0:
                price_changes.append(p_curr - p_prev)

        if price_changes:
            up_moves = sum(1 for c in price_changes if c > 0.001)
            down_moves = sum(1 for c in price_changes if c < -0.001)
            flat = sum(1 for c in price_changes if abs(c) <= 0.001)
            print(f"    {outcome}: {up_moves} price increases, {down_moves} decreases, {flat} flat")
            if up_moves > down_moves * 2:
                print(f"    → WALKING THE BOOK UP (aggressive taker, likely FOK/market orders)")
            elif down_moves > up_moves * 2:
                print(f"    → PRICES FALLING (buying on the way down, likely GTC resting)")
            else:
                print(f"    → MIXED (combination of taking and resting)")


# ── Check on-chain for a specific tx ─────────────────────────────────────────
print()
print("=" * 100)
print("ON-CHAIN ANALYSIS (Polygonscan)")
print("=" * 100)

# Pick a few representative txs to check
if btc_15m:
    sample_txs = []
    for t in btc_15m[:5]:
        tid = t.get("id", "")
        if "_" in tid:
            tx = tid.split("_")[0]
        else:
            tx = t.get("transaction_hash", "")
        if tx and tx not in [s[0] for s in sample_txs]:
            sample_txs.append((tx, t))

    for tx_hash, trade in sample_txs[:3]:
        print(f"\n  Checking tx: {tx_hash}")
        print(f"  Link: https://polygonscan.com/tx/{tx_hash}")
        print(f"  Trade: {trade.get('outcome')} {trade.get('side')} {trade.get('size')} @ ${float(trade.get('price', 0)):.4f}")

        # Try to fetch tx details from polygonscan
        # Polygonscan API (free tier)
        url = f"https://api.polygonscan.com/api?module=proxy&action=eth_getTransactionByHash&txhash={tx_hash}&apikey=YourApiKeyToken"
        try:
            r = requests.get(url, timeout=10)
            data = r.json()
            if data.get("result"):
                result = data["result"]
                input_data = result.get("input", "")
                # The CTFExchange contract method signatures:
                # fillOrder: 0xfe729aee
                # fillOrders: 0xd798eff6  (batch)
                # matchOrders: 0xe60f0c05
                method_sig = input_data[:10] if input_data else ""
                print(f"  Method signature: {method_sig}")
                print(f"  From: {result.get('from', '')}")
                print(f"  To: {result.get('to', '')}")
                print(f"  Input length: {len(input_data)} chars")

                # Known method sigs for Polymarket CTFExchange
                method_names = {
                    "0xfe729aee": "fillOrder (single order fill)",
                    "0xd798eff6": "fillOrders (batch fill multiple orders)",
                    "0xe60f0c05": "matchOrders (match maker+taker)",
                    "0x741fadcb": "cancelOrder",
                    "0x3781dbe0": "cancelOrders (batch cancel)",
                    "0xa6dfcf86": "fillOrdersGTD (GTD order fill)",
                }
                name = method_names.get(method_sig, f"unknown ({method_sig})")
                print(f"  Method: {name}")
        except Exception as e:
            print(f"  Could not fetch tx: {e}")

        time.sleep(0.3)


# ── Final inference ──────────────────────────────────────────────────────────
print()
print("=" * 100)
print("ORDER TYPE INFERENCE SUMMARY")
print("=" * 100)
print("""
What we can determine:

1. FROM FILL PATTERNS:
   - Many fills in the same second at incrementing prices → TAKER order sweeping the book
   - Single fills at exact prices → could be GTC resting order getting filled
   - Multiple fills from same order_hash → single large order partially filled across price levels

2. FROM ON-CHAIN METHOD CALLS:
   - fillOrder = single order execution (taker hitting a resting order)
   - fillOrders = batch execution (filling multiple resting orders in one tx)
   - matchOrders = maker-taker match

3. POLYMARKET CLOB ORDER TYPES:
   - GTC (Good Till Cancelled) = resting limit order, stays on book
   - GTD (Good Till Date) = limit order with expiry
   - FOK (Fill or Kill) = fill entirely NOW or cancel, never rests on book

4. KEY DISTINCTION:
   - If he's the MAKER (GTC): he places limit orders and waits for fills → earns spread
   - If he's the TAKER (FOK/market): he hits existing orders → pays spread but gets immediate fill
   - Or BOTH: places GTC resting + FOK sweeps depending on urgency
""")
