#!/usr/bin/env python3
"""Reconstruct Scallops' exact flow for the last 10-15 BTC 15m windows."""

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


def parse_ts(ts_str):
    try:
        return int(datetime.fromisoformat(ts_str.replace("Z", "+00:00")).timestamp())
    except Exception:
        return None


# ── Compute the last 15 BTC 15m window slugs from current time ──────────────
now = datetime.now(timezone.utc)
now_unix = int(now.timestamp())

# 15m windows align to 900s boundaries
current_window_start = (now_unix // 900) * 900

# Go back 15 windows (15 * 900 = 13500s = 3.75 hours)
window_starts = []
for i in range(1, 16):  # skip current (likely still open), take previous 15
    ws = current_window_start - (i * 900)
    window_starts.append(ws)

# Build slugs
slugs = [f"btc-updown-15m-{ws}" for ws in window_starts]

print(f"Current time: {now.strftime('%H:%M:%S')} UTC")
print(f"Current window start: {datetime.fromtimestamp(current_window_start, tz=timezone.utc).strftime('%H:%M:%S')}")
print(f"\nTarget windows (last 15):")
for ws in window_starts[:15]:
    dt = datetime.fromtimestamp(ws, tz=timezone.utc)
    print(f"  btc-updown-15m-{ws}  ({dt.strftime('%H:%M:%S')}-{datetime.fromtimestamp(ws+900, tz=timezone.utc).strftime('%H:%M:%S')})")

# ── Fetch Scallops' trades for these windows ────────────────────────────────
# We need to cover the full time range
earliest = min(window_starts)
latest = max(window_starts) + 900  # +15m for the window duration

print(f"\nFetching Scallops trades from {datetime.fromtimestamp(earliest, tz=timezone.utc).strftime('%H:%M:%S')} "
      f"to {datetime.fromtimestamp(latest, tz=timezone.utc).strftime('%H:%M:%S')} UTC...")

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

print(f"Total trades fetched: {len(all_trades)}")

# Filter to BTC 15m only
btc_15m = [t for t in all_trades if "btc-updown-15m" in t.get("slug", "").lower()]
print(f"BTC 15m trades: {len(btc_15m)}")

# Group by slug
by_slug = defaultdict(list)
for t in btc_15m:
    by_slug[t.get("slug", "")].append(t)

found_slugs = sorted(by_slug.keys())
print(f"Windows with trades: {len(found_slugs)}")
for s in found_slugs:
    print(f"  {s}: {len(by_slug[s])} trades")

# ── Reconstruct flow per window ─────────────────────────────────────────────
print()
print("=" * 100)
print("COMPLETE FLOW RECONSTRUCTION — Scallops BTC 15m")
print("=" * 100)

for slug in sorted(by_slug.keys()):
    trades = by_slug[slug]
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

    ws_dt = datetime.fromtimestamp(ws, tz=timezone.utc)
    we_dt = datetime.fromtimestamp(we, tz=timezone.utc)

    # Sort trades by time
    sorted_trades = sorted(trades, key=lambda x: x.get("timestamp", ""))

    # Compute cumulative positions
    up_shares = 0.0
    up_cost = 0.0
    down_shares = 0.0
    down_cost = 0.0

    print(f"\n{'=' * 100}")
    print(f"WINDOW: {slug}")
    print(f"  Time: {ws_dt.strftime('%H:%M:%S')} → {we_dt.strftime('%H:%M:%S')} UTC")
    print(f"  Trades: {len(trades)}")
    print()

    # Phase tracking
    last_outcome = None
    phase = 0
    phase_start_pct = 0

    header = (f"  {'Time':>10} | {'%Win':>5} | {'Out':>4} | {'Side':>4} | "
              f"{'Size':>8} | {'Price':>7} | {'USDC':>8} | "
              f"{'Up Shrs':>8} | {'Up Cost':>8} | {'Dn Shrs':>8} | {'Dn Cost':>8} | {'Net$':>8}")
    print(header)
    print("  " + "-" * (len(header) - 2))

    trade_log = []

    for t in sorted_trades:
        ts_str = t.get("timestamp", "")
        trade_ts = parse_ts(ts_str)
        if not trade_ts:
            continue

        pct = (trade_ts - ws) / 900 * 100
        outcome = t.get("outcome", "")
        side = t.get("side", "")
        size = float(t.get("size", 0) or 0)
        price = float(t.get("price", 0) or 0)
        usdc = size * price

        if side == "BUY":
            if outcome == "Up":
                up_shares += size
                up_cost += usdc
            else:
                down_shares += size
                down_cost += usdc
        else:  # SELL
            if outcome == "Up":
                up_shares -= size
                up_cost -= usdc
            else:
                down_shares -= size
                down_cost -= usdc

        net = up_cost - down_cost

        dt = datetime.fromtimestamp(trade_ts, tz=timezone.utc)
        time_str = dt.strftime("%H:%M:%S")

        trade_log.append({
            "time": time_str, "pct": pct, "outcome": outcome, "side": side,
            "size": size, "price": price, "usdc": usdc,
            "up_shares": up_shares, "up_cost": up_cost,
            "down_shares": down_shares, "down_cost": down_cost, "net": net,
        })

    # Print all trades (or summarize if too many)
    if len(trade_log) <= 80:
        for tl in trade_log:
            print(f"  {tl['time']:>10} | {tl['pct']:>4.1f}% | {tl['outcome']:>4} | {tl['side']:>4} | "
                  f"{tl['size']:>8.1f} | {tl['price']:>7.4f} | {tl['usdc']:>8.2f} | "
                  f"{tl['up_shares']:>8.0f} | {tl['up_cost']:>8.0f} | "
                  f"{tl['down_shares']:>8.0f} | {tl['down_cost']:>8.0f} | {tl['net']:>+8.0f}")
    else:
        # Print first 40, ..., last 20
        for tl in trade_log[:40]:
            print(f"  {tl['time']:>10} | {tl['pct']:>4.1f}% | {tl['outcome']:>4} | {tl['side']:>4} | "
                  f"{tl['size']:>8.1f} | {tl['price']:>7.4f} | {tl['usdc']:>8.2f} | "
                  f"{tl['up_shares']:>8.0f} | {tl['up_cost']:>8.0f} | "
                  f"{tl['down_shares']:>8.0f} | {tl['down_cost']:>8.0f} | {tl['net']:>+8.0f}")
        print(f"  ... ({len(trade_log) - 60} more trades) ...")
        for tl in trade_log[-20:]:
            print(f"  {tl['time']:>10} | {tl['pct']:>4.1f}% | {tl['outcome']:>4} | {tl['side']:>4} | "
                  f"{tl['size']:>8.1f} | {tl['price']:>7.4f} | {tl['usdc']:>8.2f} | "
                  f"{tl['up_shares']:>8.0f} | {tl['up_cost']:>8.0f} | "
                  f"{tl['down_shares']:>8.0f} | {tl['down_cost']:>8.0f} | {tl['net']:>+8.0f}")

    # Final summary
    final = trade_log[-1] if trade_log else None
    if final:
        total_cost = final["up_cost"] + final["down_cost"]
        up_pct = final["up_cost"] / total_cost * 100 if total_cost > 0 else 0
        dn_pct = 100 - up_pct

        # Determine which side likely won (higher price in late trades)
        late_trades = [tl for tl in trade_log if tl["pct"] >= 80]
        if late_trades:
            late_up_prices = [tl["price"] for tl in late_trades if tl["outcome"] == "Up"]
            late_dn_prices = [tl["price"] for tl in late_trades if tl["outcome"] == "Down"]
            avg_up = sum(late_up_prices) / len(late_up_prices) if late_up_prices else 0
            avg_dn = sum(late_dn_prices) / len(late_dn_prices) if late_dn_prices else 0
            likely_winner = "Up" if avg_up > avg_dn else "Down"
        else:
            # Use all trades
            all_up_p = [tl["price"] for tl in trade_log if tl["outcome"] == "Up"]
            all_dn_p = [tl["price"] for tl in trade_log if tl["outcome"] == "Down"]
            avg_up = sum(all_up_p) / len(all_up_p) if all_up_p else 0
            avg_dn = sum(all_dn_p) / len(all_dn_p) if all_dn_p else 0
            likely_winner = "Up" if avg_up > avg_dn else "Down" if avg_dn > avg_up else "?"

        up_wins_pnl = final["up_shares"] - final["up_cost"] - final["down_cost"]
        dn_wins_pnl = final["down_shares"] - final["down_cost"] - final["up_cost"]

        print()
        print(f"  SUMMARY:")
        print(f"    Up:   {final['up_shares']:>8.0f} shares | cost ${final['up_cost']:>8.0f} ({up_pct:.0f}%)")
        print(f"    Down: {final['down_shares']:>8.0f} shares | cost ${final['down_cost']:>8.0f} ({dn_pct:.0f}%)")
        print(f"    Total deployed: ${total_cost:,.0f}")
        print(f"    Likely winner: {likely_winner} (late avg: Up=${avg_up:.3f} Down=${avg_dn:.3f})")
        print(f"    If Up wins:   PnL = ${up_wins_pnl:+,.0f}")
        print(f"    If Down wins: PnL = ${dn_wins_pnl:+,.0f}")
        winner_pnl = up_wins_pnl if likely_winner == "Up" else dn_wins_pnl
        print(f"    EXPECTED PnL (if {likely_winner} wins): ${winner_pnl:+,.0f} ({winner_pnl/total_cost*100:+.1f}% ROI)" if total_cost > 0 else "")

        # Entry timing summary
        first_trade = trade_log[0]
        last_trade = trade_log[-1]
        print(f"    First entry: {first_trade['time']} ({first_trade['pct']:.1f}% through)")
        print(f"    Last entry:  {last_trade['time']} ({last_trade['pct']:.1f}% through)")

        # Phase analysis: when did they switch from one side to the other?
        phases = []
        current_side = trade_log[0]["outcome"]
        phase_start = trade_log[0]["pct"]
        phase_usdc = 0
        for tl in trade_log:
            if tl["outcome"] != current_side:
                phases.append({"side": current_side, "start": phase_start, "end": tl["pct"], "usdc": phase_usdc})
                current_side = tl["outcome"]
                phase_start = tl["pct"]
                phase_usdc = 0
            phase_usdc += tl["usdc"]
        phases.append({"side": current_side, "start": phase_start, "end": trade_log[-1]["pct"], "usdc": phase_usdc})

        print(f"    Phases:")
        for ph in phases:
            print(f"      {ph['start']:>5.1f}% → {ph['end']:>5.1f}% : {ph['side']:>4} (${ph['usdc']:,.0f})")
