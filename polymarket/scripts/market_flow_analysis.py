#!/usr/bin/env python3
"""Analyze full market order flow across all wallets to understand edge and competition."""

import json
import requests
import time
from datetime import datetime, timezone, timedelta
from collections import defaultdict, Counter

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


def parse_ts(ts_str):
    try:
        dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        return int(dt.timestamp())
    except Exception:
        return None


# ── Step 1: Get condition_ids for recent BTC 15m windows ────────────────────
print("Fetching Scallops trades for condition_ids...")
now = datetime.now(timezone.utc)
start = int((now - timedelta(hours=6)).timestamp())
end = int(now.timestamp())

all_trades = []
offset = 0
for page in range(10):
    r = requests.post(
        API_URL, headers=HEADERS, json={
            "agent_id": 556,
            "params": {"proxy_wallet": WALLET, "condition_id": "ALL",
                       "start_time": str(start), "end_time": str(end)},
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

btc_15m_conds = {}
for t in all_trades:
    slug = t.get("slug", "")
    if "btc-updown-15m" in slug.lower():
        cid = t.get("condition_id", "")
        if cid and cid not in btc_15m_conds:
            btc_15m_conds[cid] = slug

print(f"Found {len(btc_15m_conds)} unique BTC 15m windows")


# ── Step 2: For each window, get ALL wallets and analyze ────────────────────
print()
print("=" * 70)
print("FULL MARKET ORDER FLOW + SYBIL INVESTIGATION")
print("=" * 70)

# Track wallets that appear across multiple windows (sybil detection)
wallet_appearances = Counter()  # wallet -> number of windows it appears in
wallet_first_trade_pcts = defaultdict(list)  # wallet -> list of entry %s

for cid, slug in list(btc_15m_conds.items())[:8]:
    parts = slug.split("-")
    ws = None
    for p in parts:
        try:
            ts = int(p)
            if ts > 1700000000:
                ws = ts
        except ValueError:
            pass
    if not ws:
        continue
    we = ws + 900

    # Get ALL trades
    r = requests.post(
        API_URL, headers=HEADERS, json={
            "agent_id": 556,
            "params": {"proxy_wallet": "ALL", "condition_id": cid,
                       "start_time": str(ws), "end_time": str(we)},
            "pagination": {"limit": 200, "offset": 0},
            "formatter_config": {"format_type": "raw"},
        }, timeout=60)
    market_trades = extract_results(r.json())
    if not market_trades:
        time.sleep(0.3)
        continue

    # Parse
    parsed = []
    for t in market_trades:
        trade_ts = parse_ts(t.get("timestamp", ""))
        if trade_ts is None:
            continue
        pct = (trade_ts - ws) / 900 * 100
        parsed.append({
            "ts": trade_ts,
            "pct": pct,
            "outcome": t.get("outcome", ""),
            "price": float(t.get("price", 0) or 0),
            "size": float(t.get("size", 0) or 0),
            "usdc": float(t.get("size", 0) or 0) * float(t.get("price", 0) or 0),
            "wallet": t.get("proxy_wallet", ""),
        })

    if not parsed:
        time.sleep(0.3)
        continue

    # Track wallet appearances
    window_wallets = set(p["wallet"] for p in parsed)
    for w in window_wallets:
        wallet_appearances[w] += 1
        # First trade pct for this wallet in this window
        w_trades = sorted([p for p in parsed if p["wallet"] == w], key=lambda x: x["ts"])
        if w_trades:
            wallet_first_trade_pcts[w].append(w_trades[0]["pct"])

    # Determine winner
    late = [p for p in parsed if p["pct"] >= 85]
    if late:
        up_p = [p["price"] for p in late if p["outcome"] == "Up"]
        dn_p = [p["price"] for p in late if p["outcome"] == "Down"]
        winner = "Up" if (sum(up_p)/len(up_p) if up_p else 0) > (sum(dn_p)/len(dn_p) if dn_p else 0) else "Down"
    else:
        winner = "?"

    # Timing buckets with prices
    total_usdc = sum(p["usdc"] for p in parsed)
    n_wallets = len(window_wallets)
    ws_dt = datetime.fromtimestamp(ws, tz=timezone.utc)

    print(f"\n--- {slug} (winner: {winner}) | {len(parsed)} trades | {n_wallets} wallets | ${total_usdc:,.0f} ---")

    for lo, hi, label in [(0, 50, "0-50%"), (50, 70, "50-70%"), (70, 80, "70-80%"), (80, 90, "80-90%"), (90, 101, "90-100%")]:
        bucket = [p for p in parsed if lo <= p["pct"] < hi]
        if not bucket:
            continue

        win_prices = [p["price"] for p in bucket if p["outcome"] == winner]
        lose_prices = [p["price"] for p in bucket if p["outcome"] != winner]
        win_usdc = sum(p["usdc"] for p in bucket if p["outcome"] == winner)
        lose_usdc = sum(p["usdc"] for p in bucket if p["outcome"] != winner)
        bucket_usdc = win_usdc + lose_usdc

        win_avg = sum(win_prices) / len(win_prices) if win_prices else 0
        lose_avg = sum(lose_prices) / len(lose_prices) if lose_prices else 0
        edge = (1.0 - win_avg) if win_avg > 0 else 0
        n_w = len(set(p["wallet"] for p in bucket))

        print(f"  {label:>8}: {len(bucket):>3} trades ${bucket_usdc:>7,.0f} | "
              f"{n_w:>2} wallets | Winner avg ${win_avg:.3f} (edge ${edge:.3f}/share) | "
              f"Loser avg ${lose_avg:.3f}")

    time.sleep(0.3)


# ── Step 3: Sybil analysis — which wallets appear in EVERY window? ──────────
print()
print("=" * 70)
print("SYBIL INVESTIGATION: Wallets appearing across multiple windows")
print("=" * 70)

n_windows = len([v for v in btc_15m_conds.values()])
analyzed = min(8, len(btc_15m_conds))

print(f"\nAnalyzed {analyzed} windows. Wallets appearing in 3+ windows:")
print()

multi_window_wallets = [(w, c) for w, c in wallet_appearances.most_common(100) if c >= 3]
print(f"{'Wallet':<44} | Windows | Avg Entry% | Entry Range")
print("-" * 90)

scallops_count = 0
for w, count in multi_window_wallets:
    pcts = wallet_first_trade_pcts.get(w, [])
    avg_pct = sum(pcts) / len(pcts) if pcts else 0
    min_pct = min(pcts) if pcts else 0
    max_pct = max(pcts) if pcts else 0

    is_scallops = " <-- SCALLOPS" if w.lower() == WALLET.lower() else ""
    is_consistent = " ** CONSISTENT TIMING **" if max_pct - min_pct < 15 and count >= 3 else ""

    print(f"  {w[:42]} | {count:>7} | {avg_pct:>9.1f}% | {min_pct:.1f}-{max_pct:.1f}%{is_scallops}{is_consistent}")

# Group wallets by their avg entry timing to find clusters
print()
print("--- Entry Timing Clusters ---")
timing_clusters = defaultdict(list)
for w, count in multi_window_wallets:
    pcts = wallet_first_trade_pcts.get(w, [])
    avg_pct = sum(pcts) / len(pcts) if pcts else 0
    if avg_pct < 30:
        timing_clusters["early (0-30%)"].append((w, count, avg_pct))
    elif avg_pct < 60:
        timing_clusters["mid (30-60%)"].append((w, count, avg_pct))
    elif avg_pct < 80:
        timing_clusters["late (60-80%)"].append((w, count, avg_pct))
    else:
        timing_clusters["very late (80-100%)"].append((w, count, avg_pct))

for cluster, wallets in sorted(timing_clusters.items()):
    print(f"\n  {cluster}: {len(wallets)} wallets")
    for w, c, avg in wallets[:5]:
        is_scallops = " <-- SCALLOPS" if w.lower() == WALLET.lower() else ""
        print(f"    {w[:20]}... | {c} windows | avg entry {avg:.1f}%{is_scallops}")

print()
print("=" * 70)
print("CONCLUSION: Can we front-run?")
print("=" * 70)
print()
print("Key question: How many wallets consistently enter at 80-93% of window?")
late_wallets = timing_clusters.get("very late (80-100%)", [])
print(f"  Very late entrants (80-100%): {len(late_wallets)} wallets")
print(f"  If we enter at 65-75%, we beat ALL of them to the book")
print()
print("The real competition is the early/mid entrants who may also be directional:")
early = timing_clusters.get("early (0-30%)", [])
mid = timing_clusters.get("mid (30-60%)", [])
print(f"  Early (0-30%): {len(early)} wallets")
print(f"  Mid (30-60%): {len(mid)} wallets")
