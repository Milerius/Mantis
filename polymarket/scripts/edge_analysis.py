#!/usr/bin/env python3
"""Deep edge analysis: multi-wallet, single window dive, competition."""

import json
import requests
import time
from datetime import datetime, timezone, timedelta
from collections import Counter, defaultdict

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


# ── 1. Multi-wallet / Sybil info ────────────────────────────────────────────
print("=" * 70)
print("MULTI-WALLET / SYBIL ANALYSIS")
print("=" * 70)

for window in ["1", "3"]:
    r = requests.post(
        API_URL,
        headers=HEADERS,
        json={
            "agent_id": 581,
            "params": {"proxy_wallet": WALLET, "window_days": window},
            "pagination": {"limit": 10, "offset": 0},
            "formatter_config": {"format_type": "raw"},
        },
        timeout=60,
    )
    data = extract_results(r.json())
    if data:
        d = data[0]
        print(f"\n--- {window}d Window ---")
        print(f"  sybil_risk_flag: {d.get('sybil_risk_flag')}")
        print(f"  sybil_risk_score: {d.get('sybil_risk_score')}")
        print(f"  similar_wallets_count: {d.get('similar_wallets_count')}")
        print(f"  num_proxy_wallets: {d.get('num_proxy_wallets', 'N/A')}")
        print(f"  flagged_metrics: {d.get('flagged_metrics')}")
    time.sleep(0.5)


# ── 2. Get recent BTC 15m trades for deep dive ──────────────────────────────
print()
print("=" * 70)
print("SINGLE WINDOW DEEP DIVE")
print("=" * 70)

now = datetime.now(timezone.utc)
start = int((now - timedelta(hours=3)).timestamp())
end = int(now.timestamp())

all_trades = []
offset = 0
for page in range(15):
    r = requests.post(
        API_URL,
        headers=HEADERS,
        json={
            "agent_id": 556,
            "params": {
                "proxy_wallet": WALLET,
                "condition_id": "ALL",
                "start_time": str(start),
                "end_time": str(end),
            },
            "pagination": {"limit": 200, "offset": offset},
            "formatter_config": {"format_type": "raw"},
        },
        timeout=60,
    )
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
print(f"BTC 15m trades (3h): {len(btc_15m)}")

# Group by window
windows = defaultdict(list)
for t in btc_15m:
    windows[t.get("slug", "")].append(t)

if not windows:
    print("No BTC 15m windows found!")
    exit()

# Pick window with most trades
top_slug = max(windows, key=lambda s: len(windows[s]))
trades = windows[top_slug]

parts = top_slug.split("-")
ws = None
for p in parts:
    try:
        ts = int(p)
        if ts > 1700000000:
            ws = ts
    except ValueError:
        pass

we = ws + 900  # 15m
ws_dt = datetime.fromtimestamp(ws, tz=timezone.utc)
we_dt = datetime.fromtimestamp(we, tz=timezone.utc)

print(f"\n--- Window: {top_slug} ---")
print(f"  Open: {ws_dt.strftime('%H:%M:%S')} | Close: {we_dt.strftime('%H:%M:%S')} UTC")
print(f"  Total trades: {len(trades)}")

# Sort and analyze tick-by-tick
sorted_t = sorted(trades, key=lambda x: x.get("timestamp", ""))

up_shares = 0
up_cost = 0
down_shares = 0
down_cost = 0

print(f"\n  {'Time':>10} | {'Out':>4} | {'Side':>4} | {'Size':>8} | {'Price':>7} | {'USDC':>8} | {'%Win':>5} | {'CumUp$':>7} | {'CumDn$':>7} | {'NetPos':>8}")
print("  " + "-" * 100)

for i, t in enumerate(sorted_t):
    ts_str = t.get("timestamp", "")
    try:
        dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        trade_ts = int(dt.timestamp())
        pct = (trade_ts - ws) / 900 * 100
    except Exception:
        pct = 0
        dt = None

    outcome = t.get("outcome", "")
    size = float(t.get("size", 0) or 0)
    price = float(t.get("price", 0) or 0)
    usdc = size * price

    if outcome == "Up":
        up_shares += size
        up_cost += usdc
    else:
        down_shares += size
        down_cost += usdc

    net = up_cost - down_cost

    if i < 60 or i >= len(sorted_t) - 10:
        time_str = dt.strftime("%H:%M:%S") if dt else "?"
        print(
            f"  {time_str:>10} | {outcome:>4} | {t.get('side',''):>4} | {size:>8.1f} | "
            f"{price:>7.4f} | {usdc:>8.2f} | {pct:>4.1f}% | {up_cost:>7.0f} | "
            f"{down_cost:>7.0f} | {net:>+8.0f}"
        )
    elif i == 60:
        print(f"  ... ({len(sorted_t) - 70} more trades) ...")

print(f"\n  FINAL POSITION:")
print(f"    Up: {up_shares:.0f} shares, cost ${up_cost:.0f}")
print(f"    Down: {down_shares:.0f} shares, cost ${down_cost:.0f}")
print(f"    Total deployed: ${up_cost + down_cost:.0f}")
heavier = "Up" if up_cost > down_cost else "Down"
heavier_pct = max(up_cost, down_cost) / max(up_cost + down_cost, 1) * 100
print(f"    Capital skew: {heavier} heavy ({heavier_pct:.0f}%)")

up_wins_pnl = up_shares - up_cost - down_cost
down_wins_pnl = down_shares - down_cost - up_cost
print(f"\n  If Up wins:  PnL = ${up_wins_pnl:+,.0f}  (redeem {up_shares:.0f} Up shares at $1)")
print(f"  If Down wins: PnL = ${down_wins_pnl:+,.0f}  (redeem {down_shares:.0f} Down shares at $1)")


# ── 3. Competition: ALL wallets in same window ───────────────────────────────
print()
print("=" * 70)
print("COMPETITION ANALYSIS — ALL wallets in same window")
print("=" * 70)

sample = windows[top_slug][0]
cond_id = sample.get("condition_id", "")
print(f"Checking condition_id: {cond_id[:30]}...")
print(f"Window: {top_slug}")

r = requests.post(
    API_URL,
    headers=HEADERS,
    json={
        "agent_id": 556,
        "params": {
            "proxy_wallet": "ALL",
            "condition_id": cond_id,
            "start_time": str(ws),
            "end_time": str(we),
        },
        "pagination": {"limit": 200, "offset": 0},
        "formatter_config": {"format_type": "raw"},
    },
    timeout=60,
)
all_market_trades = extract_results(r.json())
print(f"Total trades in this market (all wallets): {len(all_market_trades)}")

if all_market_trades:
    wallet_counter = Counter(t.get("proxy_wallet", "") for t in all_market_trades)
    print(f"Unique wallets: {len(wallet_counter)}")

    print(f"\nTop wallets by trade count:")
    for w, count in wallet_counter.most_common(15):
        w_trades = [t for t in all_market_trades if t.get("proxy_wallet", "") == w]
        w_usdc = sum(
            float(t.get("size", 0)) * float(t.get("price", 0)) for t in w_trades
        )
        w_up = sum(1 for t in w_trades if t.get("outcome") == "Up")
        w_down = sum(1 for t in w_trades if t.get("outcome") == "Down")
        is_scallops = " <-- SCALLOPS" if w.lower() == WALLET.lower() else ""
        print(
            f"  {w[:16]}... : {count:>3} trades, ${w_usdc:>8,.0f} | Up:{w_up} Down:{w_down}{is_scallops}"
        )

    # Scallops share
    scallops_trades = [
        t
        for t in all_market_trades
        if t.get("proxy_wallet", "").lower() == WALLET.lower()
    ]
    scallops_usdc = sum(
        float(t.get("size", 0)) * float(t.get("price", 0)) for t in scallops_trades
    )
    total_usdc = sum(
        float(t.get("size", 0)) * float(t.get("price", 0)) for t in all_market_trades
    )
    print(
        f"\nScallops market share: {len(scallops_trades)}/{len(all_market_trades)} trades "
        f"({len(scallops_trades) / max(len(all_market_trades), 1) * 100:.1f}%)"
    )
    print(
        f"Scallops USDC share: ${scallops_usdc:,.0f}/${total_usdc:,.0f} "
        f"({scallops_usdc / max(total_usdc, 1) * 100:.1f}%)"
    )

    # Check: are any of the other wallets possibly Scallops sybils?
    # Look for wallets with similar trading patterns (same timing, same sides)
    print(f"\n--- Sybil Detection: wallets with similar patterns ---")
    scallops_first_ts = None
    for t in sorted(scallops_trades, key=lambda x: x.get("timestamp", "")):
        try:
            dt = datetime.fromisoformat(t["timestamp"].replace("Z", "+00:00"))
            scallops_first_ts = int(dt.timestamp())
            break
        except Exception:
            pass

    if scallops_first_ts:
        for w, count in wallet_counter.most_common(20):
            if w.lower() == WALLET.lower():
                continue
            w_trades = sorted(
                [t for t in all_market_trades if t.get("proxy_wallet", "") == w],
                key=lambda x: x.get("timestamp", ""),
            )
            if not w_trades:
                continue
            try:
                first_dt = datetime.fromisoformat(
                    w_trades[0]["timestamp"].replace("Z", "+00:00")
                )
                first_ts = int(first_dt.timestamp())
                delay_from_scallops = first_ts - scallops_first_ts
                delay_from_open = first_ts - ws

                w_outcomes = Counter(t.get("outcome", "") for t in w_trades)
                w_usdc = sum(
                    float(t.get("size", 0)) * float(t.get("price", 0))
                    for t in w_trades
                )

                if abs(delay_from_scallops) < 5:
                    flag = " *** NEAR-SIMULTANEOUS WITH SCALLOPS ***"
                elif abs(delay_from_scallops) < 30:
                    flag = " * close timing"
                else:
                    flag = ""

                print(
                    f"  {w[:16]}... | {count:>3} trades ${w_usdc:>6,.0f} | "
                    f"+{delay_from_open}s from open | "
                    f"{delay_from_scallops:+d}s vs Scallops | "
                    f"Up:{w_outcomes.get('Up', 0)} Dn:{w_outcomes.get('Down', 0)}{flag}"
                )
            except Exception:
                pass

# Also check a second window for comparison
print()
print("=" * 70)
print("SECOND WINDOW COMPARISON")
print("=" * 70)

second_slug = sorted(windows.keys(), key=lambda s: len(windows[s]), reverse=True)
if len(second_slug) > 1:
    second_slug = second_slug[1]
    trades2 = windows[second_slug]
    sample2 = trades2[0]
    cond_id2 = sample2.get("condition_id", "")

    parts2 = second_slug.split("-")
    ws2 = None
    for p in parts2:
        try:
            ts = int(p)
            if ts > 1700000000:
                ws2 = ts
        except ValueError:
            pass

    we2 = ws2 + 900
    print(f"Window: {second_slug}")

    r = requests.post(
        API_URL,
        headers=HEADERS,
        json={
            "agent_id": 556,
            "params": {
                "proxy_wallet": "ALL",
                "condition_id": cond_id2,
                "start_time": str(ws2),
                "end_time": str(we2),
            },
            "pagination": {"limit": 200, "offset": 0},
            "formatter_config": {"format_type": "raw"},
        },
        timeout=60,
    )
    all_market_trades2 = extract_results(r.json())
    print(f"Total trades (all wallets): {len(all_market_trades2)}")

    if all_market_trades2:
        wallet_counter2 = Counter(
            t.get("proxy_wallet", "") for t in all_market_trades2
        )
        print(f"Unique wallets: {len(wallet_counter2)}")

        for w, count in wallet_counter2.most_common(10):
            w_usdc = sum(
                float(t.get("size", 0)) * float(t.get("price", 0))
                for t in all_market_trades2
                if t.get("proxy_wallet", "") == w
            )
            is_scallops = " <-- SCALLOPS" if w.lower() == WALLET.lower() else ""
            print(f"  {w[:16]}... : {count:>3} trades, ${w_usdc:>8,.0f}{is_scallops}")

        scallops2 = [
            t
            for t in all_market_trades2
            if t.get("proxy_wallet", "").lower() == WALLET.lower()
        ]
        total_usdc2 = sum(
            float(t.get("size", 0)) * float(t.get("price", 0))
            for t in all_market_trades2
        )
        scallops_usdc2 = sum(
            float(t.get("size", 0)) * float(t.get("price", 0)) for t in scallops2
        )
        print(
            f"\nScallops share: {len(scallops2)}/{len(all_market_trades2)} trades, "
            f"${scallops_usdc2:,.0f}/${total_usdc2:,.0f} "
            f"({scallops_usdc2 / max(total_usdc2, 1) * 100:.1f}%)"
        )
