#!/usr/bin/env python3
"""
Fetch 20 more BTC 15m windows for Scallops trader going further back in time.
Windows 03:30-08:45 UTC on 2026-04-02 (before the existing 08:45-15:30 set).
"""

import json
import requests
import time
from datetime import datetime, timezone
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
    if isinstance(result, list):
        return result
    return []


def has_more_pages(result):
    if isinstance(result, dict):
        pag = result.get("pagination", {})
        return pag.get("has_more", False)
    return False


def api_call(payload, retries=2):
    for attempt in range(retries + 1):
        try:
            resp = requests.post(API_URL, headers=HEADERS, json=payload, timeout=60)
            if resp.status_code == 200:
                return resp.json()
            elif resp.status_code == 429:
                wait = 5 * (attempt + 1)
                print(f"  Rate limited, waiting {wait}s...")
                time.sleep(wait)
            else:
                print(f"  API error {resp.status_code}: {resp.text[:200]}")
                if attempt < retries:
                    time.sleep(2)
        except Exception as e:
            print(f"  Request error: {e}")
            if attempt < retries:
                time.sleep(2)
    return None


# ── Compute time range ────────────────────────────────────────────────────────
# 2026-04-02 08:45 UTC = boundary. We want 20 windows BEFORE that.
# 08:45 UTC on 2026-04-02
boundary = datetime(2026, 4, 2, 8, 45, 0, tzinfo=timezone.utc)
boundary_unix = int(boundary.timestamp())

# 20 windows of 15m = 5 hours back => 03:45 UTC
# Add some buffer on both sides for trades that may land slightly outside
start_unix = boundary_unix - (20 * 900) - 300  # ~03:30 with buffer
end_unix = boundary_unix + 300  # small buffer after 08:45

print(f"Fetching trades from {datetime.fromtimestamp(start_unix, tz=timezone.utc)} "
      f"to {datetime.fromtimestamp(end_unix, tz=timezone.utc)}")
print(f"  start_unix={start_unix}  end_unix={end_unix}")

# ── Fetch ALL trades with pagination ──────────────────────────────────────────
all_trades = []
offset = 0
page = 0
max_pages = 200

while page < max_pages:
    result = api_call({
        "agent_id": 556,
        "params": {
            "proxy_wallet": WALLET,
            "condition_id": "ALL",
            "start_time": str(start_unix),
            "end_time": str(end_unix),
        },
        "pagination": {"limit": 200, "offset": offset},
        "formatter_config": {"format_type": "raw"},
    })
    trades = extract_results(result)
    if trades:
        all_trades.extend(trades)
        print(f"  Page {page}, offset {offset}: +{len(trades)} trades (total: {len(all_trades)})")

    if not has_more_pages(result):
        break
    offset += 200
    page += 1
    time.sleep(0.2)

print(f"\nTotal raw trades fetched: {len(all_trades)}")

# ── Filter to btc-updown-15m only ────────────────────────────────────────────
btc_15m = [t for t in all_trades if "btc-updown-15m" in t.get("slug", "").lower()]
print(f"BTC 15m trades: {len(btc_15m)}")

# ── Group by slug ─────────────────────────────────────────────────────────────
by_slug = defaultdict(list)
for t in btc_15m:
    by_slug[t["slug"]].append(t)

print(f"Unique BTC 15m windows: {len(by_slug)}")
for slug in sorted(by_slug.keys()):
    print(f"  {slug}: {len(by_slug[slug])} trades")


# ── Helper: parse trade timestamp ─────────────────────────────────────────────
def parse_ts(trade):
    ts_str = trade.get("timestamp", "")
    if not ts_str:
        return None
    try:
        dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        return int(dt.timestamp())
    except Exception:
        return None


def get_window_start(slug):
    """Extract window start unix from slug like btc-updown-15m-1774953900."""
    parts = slug.split("-")
    for p in parts:
        try:
            ts = int(p)
            if ts > 1700000000:
                return ts
        except ValueError:
            pass
    return None


# ── Analyze each window ───────────────────────────────────────────────────────
results = []

for slug in sorted(by_slug.keys()):
    trades = by_slug[slug]
    window_start = get_window_start(slug)
    if window_start is None:
        continue

    # Only include windows strictly before 08:45 boundary
    if window_start >= boundary_unix:
        continue

    window_end = window_start + 900
    time_str = datetime.fromtimestamp(window_start, tz=timezone.utc).strftime("%H:%M")

    # Separate by outcome
    up_trades = [t for t in trades if t.get("outcome", "").lower() == "up"]
    down_trades = [t for t in trades if t.get("outcome", "").lower() == "down"]

    up_shares = sum(float(t.get("size", 0) or 0) for t in up_trades)
    up_cost = sum(float(t.get("size", 0) or 0) * float(t.get("price", 0) or 0) for t in up_trades)
    down_shares = sum(float(t.get("size", 0) or 0) for t in down_trades)
    down_cost = sum(float(t.get("size", 0) or 0) * float(t.get("price", 0) or 0) for t in down_trades)

    total_cost = up_cost + down_cost
    if total_cost < 1.0:
        continue  # skip windows with negligible activity

    # Determine winner from late trades (75%+ through window)
    late_up_shares = 0.0
    late_down_shares = 0.0
    for t in trades:
        ts = parse_ts(t)
        if ts is None:
            continue
        pct_through = (ts - window_start) / 900.0
        if pct_through >= 0.75:
            size = float(t.get("size", 0) or 0)
            outcome = t.get("outcome", "").lower()
            if outcome == "up":
                late_up_shares += size
            elif outcome == "down":
                late_down_shares += size

    # If no late trades, use overall dominant side
    if late_up_shares == 0 and late_down_shares == 0:
        winner = "Up" if up_shares >= down_shares else "Down"
    else:
        winner = "Up" if late_up_shares >= late_down_shares else "Down"

    # Winner/loser stats
    if winner == "Up":
        w_trades, l_trades = up_trades, down_trades
        w_shares, l_shares = up_shares, down_shares
        w_cost, l_cost = up_cost, down_cost
    else:
        w_trades, l_trades = down_trades, up_trades
        w_shares, l_shares = down_shares, up_shares
        w_cost, l_cost = down_cost, up_cost

    # Weighted average prices
    w_prices = [float(t.get("price", 0) or 0) for t in w_trades]
    l_prices = [float(t.get("price", 0) or 0) for t in l_trades]
    w_sizes = [float(t.get("size", 0) or 0) for t in w_trades]
    l_sizes = [float(t.get("size", 0) or 0) for t in l_trades]

    w_avg = sum(p * s for p, s in zip(w_prices, w_sizes)) / max(sum(w_sizes), 0.001)
    l_avg = sum(p * s for p, s in zip(l_prices, l_sizes)) / max(sum(l_sizes), 0.001) if sum(l_sizes) > 0 else 0.0

    # Expensive %: winner fills above $0.80
    expensive_count = sum(1 for p in w_prices if p > 0.80)
    expensive_pct = expensive_count / max(len(w_prices), 1) * 100

    # Early correct: did the first few trades (first 25%) bet on the winner?
    early_trades = []
    for t in trades:
        ts = parse_ts(t)
        if ts is not None:
            early_trades.append((ts, t))
    early_trades.sort(key=lambda x: x[0])
    n_early = max(1, len(early_trades) // 4)
    early_winner_count = sum(1 for _, t in early_trades[:n_early]
                            if t.get("outcome", "").lower() == winner.lower())
    early_correct = early_winner_count >= (n_early / 2)

    # Switches: count side switches in chronological order
    sorted_trades = sorted(trades, key=lambda t: parse_ts(t) or 0)
    switches = 0
    prev_outcome = None
    for t in sorted_trades:
        outcome = t.get("outcome", "").lower()
        if prev_outcome is not None and outcome != prev_outcome:
            switches += 1
        prev_outcome = outcome

    # PnL = winner_shares - total_cost
    pnl = w_shares - total_cost
    roi = (pnl / total_cost * 100) if total_cost > 0 else 0.0

    # Dominance %
    dom_pct = (w_cost / total_cost * 100) if total_cost > 0 else 0.0

    results.append({
        "time_str": time_str,
        "winner": winner,
        "total": round(total_cost, 2),
        "pnl": round(pnl, 2),
        "roi": round(roi, 1),
        "w_avg": round(w_avg, 3),
        "l_avg": round(l_avg, 3),
        "expensive_pct": round(expensive_pct, 1),
        "early_correct": early_correct,
        "switches": switches,
        "dom_pct": round(dom_pct, 1),
        "slug": slug,
        "n_trades": len(trades),
    })

# Sort by time
results.sort(key=lambda r: r["time_str"])

# ── Print as copy-pasteable Python tuples ─────────────────────────────────────
print("\n" + "=" * 90)
print("RESULTS: 20 extra windows (03:30-08:45 UTC)")
print("(time, winner, total, pnl, roi%, w_avg, l_avg, expensive%, early_correct, switches, dom%)")
print("=" * 90)

print("\nextra_windows = [")
for r in results:
    print(f'    ("{r["time_str"]}", "{r["winner"]}", {r["total"]}, {r["pnl"]}, {r["roi"]}, '
          f'{r["w_avg"]}, {r["l_avg"]}, {r["expensive_pct"]}, {r["early_correct"]}, '
          f'{r["switches"]}, {r["dom_pct"]}),  # {r["n_trades"]} trades')
print("]")

# ── Summary stats ─────────────────────────────────────────────────────────────
if results:
    total_pnl = sum(r["pnl"] for r in results)
    total_invested = sum(r["total"] for r in results)
    wins = sum(1 for r in results if r["pnl"] > 0)
    losses = sum(1 for r in results if r["pnl"] <= 0)
    avg_roi = sum(r["roi"] for r in results) / len(results)
    print(f"\nSummary: {len(results)} windows, {wins}W/{losses}L")
    print(f"  Total PnL: ${total_pnl:.2f}")
    print(f"  Total invested: ${total_invested:.2f}")
    print(f"  Overall ROI: {total_pnl/max(total_invested,0.01)*100:.1f}%")
    print(f"  Avg ROI per window: {avg_roi:.1f}%")
    print(f"  Avg w_avg: {sum(r['w_avg'] for r in results)/len(results):.3f}")
    print(f"  Avg expensive%: {sum(r['expensive_pct'] for r in results)/len(results):.1f}%")

# ── Write JSON ────────────────────────────────────────────────────────────────
json_path = "/Users/milerius/Documents/Mantis/polymarket/scripts/extra_windows.json"
with open(json_path, "w") as f:
    json.dump(results, f, indent=2)
print(f"\nJSON written to {json_path}")
