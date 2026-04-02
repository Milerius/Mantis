#!/usr/bin/env python3
"""Deep dive into the last 4 BTC 15m windows with full tx details."""

import json
import requests
import time
from datetime import datetime, timezone

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


now = datetime.now(timezone.utc)
now_unix = int(now.timestamp())
current_ws = (now_unix // 900) * 900

# Last 4 CLOSED 15m windows
targets = []
for i in range(1, 5):
    ws = current_ws - (i * 900)
    targets.append(ws)

print(f"Current time: {now.strftime('%Y-%m-%d %H:%M:%S')} UTC")
print(f"Analyzing last 4 closed BTC 15m windows:\n")
for ws in targets:
    dt = datetime.fromtimestamp(ws, tz=timezone.utc)
    dt_end = datetime.fromtimestamp(ws + 900, tz=timezone.utc)
    print(f"  btc-updown-15m-{ws}  ({dt.strftime('%H:%M:%S')} - {dt_end.strftime('%H:%M:%S')})")

# Fetch trades covering all 4 windows
earliest = min(targets)
latest = max(targets) + 900

print(f"\nFetching trades...")
all_trades = []
offset = 0
for page in range(50):
    r = requests.post(API_URL, headers=HEADERS, json={
        "agent_id": 556,
        "params": {
            "proxy_wallet": WALLET,
            "condition_id": "ALL",
            "start_time": str(earliest),
            "end_time": str(latest),
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

print(f"Fetched {len(all_trades)} trades total")

# Group by slug, filter BTC 15m
from collections import defaultdict
by_slug = defaultdict(list)
for t in all_trades:
    slug = t.get("slug", "")
    if "btc-updown-15m" in slug.lower():
        by_slug[slug].append(t)

print(f"BTC 15m windows found: {len(by_slug)}")

# Process each target window
for ws in targets:
    slug = f"btc-updown-15m-{ws}"
    trades = by_slug.get(slug, [])
    we = ws + 900
    ws_dt = datetime.fromtimestamp(ws, tz=timezone.utc)
    we_dt = datetime.fromtimestamp(we, tz=timezone.utc)

    print("\n")
    print("#" * 120)
    print(f"## WINDOW: {slug}")
    print(f"## {ws_dt.strftime('%Y-%m-%d %H:%M:%S')} UTC  -->  {we_dt.strftime('%H:%M:%S')} UTC")
    print(f"## Trades: {len(trades)}")
    print("#" * 120)

    if not trades:
        print("  NO TRADES IN THIS WINDOW")
        continue

    sorted_trades = sorted(trades, key=lambda x: x.get("timestamp", ""))

    # Cumulative tracking
    up_shares = 0.0
    up_cost = 0.0
    down_shares = 0.0
    down_cost = 0.0

    print()
    print(f"  {'#':>3} | {'Time':>10} | {'%Win':>5} | {'Out':>4} | {'Side':>4} | "
          f"{'Size':>9} | {'Price':>7} | {'USDC':>9} | "
          f"{'Up$':>7} | {'Dn$':>7} | {'TxHash'}")
    print("  " + "-" * 115)

    for i, t in enumerate(sorted_trades):
        ts_str = t.get("timestamp", "")
        try:
            dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
            trade_ts = int(dt.timestamp())
            pct = (trade_ts - ws) / 900 * 100
        except Exception:
            pct = 0
            dt = None

        outcome = t.get("outcome", "")
        side = t.get("side", "")
        size = float(t.get("size", 0) or 0)
        price = float(t.get("price", 0) or 0)
        usdc = size * price
        tx = t.get("transaction_hash", t.get("id", ""))

        if side == "BUY":
            if outcome == "Up":
                up_shares += size
                up_cost += usdc
            else:
                down_shares += size
                down_cost += usdc
        else:
            if outcome == "Up":
                up_shares -= size
                up_cost -= usdc
            else:
                down_shares -= size
                down_cost -= usdc

        time_str = dt.strftime("%H:%M:%S") if dt else "?"

        # Shorten tx for display, keep full for link
        tx_short = tx[:20] + "..." if len(tx) > 20 else tx

        print(f"  {i+1:>3} | {time_str:>10} | {pct:>4.1f}% | {outcome:>4} | {side:>4} | "
              f"{size:>9.2f} | ${price:>6.4f} | ${usdc:>8.2f} | "
              f"${up_cost:>6.0f} | ${down_cost:>6.0f} | {tx_short}")

    # Final summary
    total_cost = up_cost + down_cost
    if total_cost <= 0:
        continue

    up_pct = up_cost / total_cost * 100
    dn_pct = 100 - up_pct

    up_avg_price = up_cost / up_shares if up_shares > 0 else 0
    dn_avg_price = down_cost / down_shares if down_shares > 0 else 0

    up_wins_pnl = up_shares - up_cost - down_cost
    dn_wins_pnl = down_shares - down_cost - up_cost

    # Infer winner from late prices
    late = [t for t in sorted_trades if True]  # get last few
    late_up = [float(t.get("price", 0)) for t in sorted_trades[-20:] if t.get("outcome") == "Up"]
    late_dn = [float(t.get("price", 0)) for t in sorted_trades[-20:] if t.get("outcome") == "Down"]
    avg_up_late = sum(late_up) / len(late_up) if late_up else 0
    avg_dn_late = sum(late_dn) / len(late_dn) if late_dn else 0
    likely_winner = "Up" if avg_up_late > avg_dn_late else "Down"

    print()
    print(f"  {'=' * 80}")
    print(f"  POSITION SUMMARY")
    print(f"  {'=' * 80}")
    print(f"  Up:    {up_shares:>8.0f} shares | avg price ${up_avg_price:.4f} | cost ${up_cost:>8,.0f} ({up_pct:.0f}%)")
    print(f"  Down:  {down_shares:>8.0f} shares | avg price ${dn_avg_price:.4f} | cost ${down_cost:>8,.0f} ({dn_pct:.0f}%)")
    print(f"  Total: ${total_cost:>8,.0f} deployed")
    print()
    print(f"  Likely winner: {likely_winner} (last trades avg: Up=${avg_up_late:.3f} Down=${avg_dn_late:.3f})")
    print()
    print(f"  If Up wins:   redeem {up_shares:.0f} × $1 = ${up_shares:,.0f} → PnL = ${up_wins_pnl:>+,.0f} ({up_wins_pnl/total_cost*100:>+.1f}%)")
    print(f"  If Down wins: redeem {down_shares:.0f} × $1 = ${down_shares:,.0f} → PnL = ${dn_wins_pnl:>+,.0f} ({dn_wins_pnl/total_cost*100:>+.1f}%)")

    expected_pnl = up_wins_pnl if likely_winner == "Up" else dn_wins_pnl
    print(f"  Expected ({likely_winner} wins): ${expected_pnl:>+,.0f} ({expected_pnl/total_cost*100:>+.1f}%)")

    # Price analysis
    print()
    print(f"  {'=' * 80}")
    print(f"  PRICE ANALYSIS")
    print(f"  {'=' * 80}")

    # By timing bucket
    for lo, hi, label in [(0, 20, "First 20%"), (20, 50, "20-50%"), (50, 80, "50-80%"), (80, 101, "Last 20%")]:
        bucket_trades = []
        for t in sorted_trades:
            ts_str = t.get("timestamp", "")
            try:
                dt2 = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
                p = (int(dt2.timestamp()) - ws) / 900 * 100
                if lo <= p < hi:
                    bucket_trades.append(t)
            except Exception:
                pass

        if not bucket_trades:
            continue

        up_p = [float(t["price"]) for t in bucket_trades if t.get("outcome") == "Up" and float(t.get("price", 0)) > 0]
        dn_p = [float(t["price"]) for t in bucket_trades if t.get("outcome") == "Down" and float(t.get("price", 0)) > 0]
        up_usdc = sum(float(t.get("size", 0)) * float(t.get("price", 0)) for t in bucket_trades if t.get("outcome") == "Up")
        dn_usdc = sum(float(t.get("size", 0)) * float(t.get("price", 0)) for t in bucket_trades if t.get("outcome") == "Down")

        up_avg = sum(up_p) / len(up_p) if up_p else 0
        dn_avg = sum(dn_p) / len(dn_p) if dn_p else 0

        print(f"  {label:>12}: {len(bucket_trades):>3} trades | "
              f"Up avg ${up_avg:.3f} ({len(up_p)} fills, ${up_usdc:,.0f}) | "
              f"Dn avg ${dn_avg:.3f} ({len(dn_p)} fills, ${dn_usdc:,.0f})")

    # Tx links
    print()
    print(f"  {'=' * 80}")
    print(f"  TRANSACTION LINKS (polygonscan)")
    print(f"  {'=' * 80}")
    seen_tx = set()
    for t in sorted_trades:
        tx = t.get("transaction_hash", "")
        # The id field sometimes has tx_hash embedded
        if not tx:
            tid = t.get("id", "")
            if tid and tid.startswith("0x"):
                tx = tid.split("_")[0]
        if tx and tx not in seen_tx:
            seen_tx.add(tx)
            print(f"  https://polygonscan.com/tx/{tx}")

    print(f"\n  Condition ID: {sorted_trades[0].get('condition_id', 'N/A')}")
    print(f"  Market slug: {slug}")
    print(f"  Polymarket: https://polymarket.com/event/{slug}")
