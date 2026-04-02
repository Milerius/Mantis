#!/usr/bin/env python3
"""
BTC 5m/15m Trader Report Generator for Idolized-Scallops.
Fetches trade data from Heisenberg API and generates a complete analysis report.

Usage:
    python scripts/btc_trader_report.py [--days 3] [--wallet 0x...] [--output report.md]
"""

import json
import requests
import time
import argparse
import base64
import sys
from datetime import datetime, timezone, timedelta
from collections import defaultdict, Counter
from pathlib import Path

# ── Configuration ────────────────────────────────────────────────────────────

API_URL = "https://narrative.agent.heisenberg.so/api/v2/semantic/retrieve/parameterized"
DEFAULT_WALLET = "0xe1d6b51521bd4365769199f392f9818661bd907c"
DEFAULT_API_KEY = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ0b2tlbl90eXBlIjoiYWNjZXNzIiwiZXhwIjoxNzgwMzIyOTcyLCJpYXQiOjE3NzUxMzg5NzIsImp0aSI6ImNiMWJkZTk4Y2FlYTRhMTg5NmVjMzVkMGMxMWJlMjcyIiwidXNlcl9pZCI6MTI0OCwic2NvcGUiOiJsYXVuY2hwYWQ6YWdlbnQtcmVhZCxyZXRyaWV2ZXI6ZWNoby1nZW5lcmF0aW9uLHJldHJpZXZlcjpmZWF0dXJlLWV4dHJhY3Rpb24sdXNlcjpyZWFkLHJldHJpZXZlcjphZ2VudC1vcHRpb24tcmV0cmlldmFsLGxhdW5jaHBhZDphZ2VudC1jcmVhdGlvbixsYXVuY2hwYWQ6YWdlbnQtdXBkYXRlLHVzZXI6d3JpdGUscmV0cmlldmVyOnNlbWFudGljLXJldHJpZXZhbCxsYXVuY2hwYWQ6ZWNoby1zdHlsZS1jcmVhdGlvbiIsInRva2VuX25hbWUiOiJiYXNlX2xvZ2luIn0.L3O49KE6uFtqkXGaXQrEA6lmBKlnjzGatd9niJ3CRkM"

# Agent IDs
AGENT_TRADES = 556
AGENT_WALLET360 = 581
AGENT_PNL = 569
AGENT_LEADERBOARD = 584


# ── API Helpers ──────────────────────────────────────────────────────────────

def make_headers(api_key):
    return {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }


def api_call(payload, headers, retries=2):
    for attempt in range(retries + 1):
        try:
            resp = requests.post(API_URL, headers=headers, json=payload, timeout=60)
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


def extract_results(result):
    """Extract results list from API response (handles multiple response shapes)."""
    if not result:
        return []
    if isinstance(result, dict):
        if 'data' in result and isinstance(result['data'], dict):
            if 'results' in result['data']:
                return result['data']['results']
        if 'data' in result and isinstance(result['data'], list):
            return result['data']
        if 'results' in result:
            return result['results']
    if isinstance(result, list):
        return result
    return []


def has_more_pages(result):
    if isinstance(result, dict):
        pag = result.get('pagination', {})
        return pag.get('has_more', False)
    return False


# ── Data Fetchers ────────────────────────────────────────────────────────────

def fetch_all_trades(wallet, start_unix, end_unix, headers, page_size=200):
    """Fetch all trades with full pagination (max page_size=200 per API docs)."""
    all_trades = []
    offset = 0
    page = 0
    max_pages = 5000

    while page < max_pages:
        result = api_call({
            "agent_id": AGENT_TRADES,
            "params": {
                "proxy_wallet": wallet,
                "condition_id": "ALL",
                "start_time": str(start_unix),
                "end_time": str(end_unix),
            },
            "pagination": {"limit": page_size, "offset": offset},
            "formatter_config": {"format_type": "raw"},
        }, headers)

        trades = extract_results(result)
        if trades:
            all_trades.extend(trades)
            if page % 10 == 0:
                print(f"  Page {page}, offset {offset}: {len(all_trades)} trades so far")

        if not has_more_pages(result):
            break
        offset += page_size
        page += 1
        time.sleep(0.15)

    return all_trades


def fetch_wallet360(wallet, window_days, headers):
    """Fetch Wallet 360 metrics."""
    result = api_call({
        "agent_id": AGENT_WALLET360,
        "params": {
            "proxy_wallet": wallet,
            "window_days": str(window_days),
        },
        "pagination": {"limit": 10, "offset": 0},
        "formatter_config": {"format_type": "raw"},
    }, headers)
    data = extract_results(result)
    return data[0] if data else None


def fetch_pnl(wallet, start_date, end_date, granularity, headers):
    """Fetch PnL data."""
    result = api_call({
        "agent_id": AGENT_PNL,
        "params": {
            "wallet": wallet,
            "granularity": granularity,
            "start_time": start_date,
            "end_time": end_date,
        },
        "formatter_config": {"format_type": "raw"},
    }, headers)
    return extract_results(result)


# ── Trade Filters ────────────────────────────────────────────────────────────

def is_btc_trade(trade):
    """Check if trade is a BTC Up/Down market."""
    slug = trade.get('slug', '').lower()
    return slug.startswith('btc-updown') or slug.startswith('bitcoin-')


def is_5m_or_15m(trade):
    """Check if trade is on 5m or 15m timeframe."""
    slug = trade.get('slug', '').lower()
    parts = slug.split('-')
    return any(p in ('5m', '15m') for p in parts)


def get_timeframe(trade):
    slug = trade.get('slug', '').lower()
    parts = slug.split('-')
    for p in parts:
        if p in ('5m', '15m', '1h', '4h'):
            return p
    return 'unknown'


def get_window_start_ts(trade):
    """Extract window START timestamp from slug (e.g., btc-updown-15m-1774953900).
    The slug timestamp is the window open time, NOT the close time."""
    slug = trade.get('slug', '')
    parts = slug.split('-')
    for p in parts:
        try:
            ts = int(p)
            if ts > 1700000000:
                return ts
        except ValueError:
            pass
    return None


def get_trade_timestamp(trade):
    """Parse trade timestamp to unix seconds."""
    ts_str = trade.get('timestamp', '')
    if not ts_str:
        return None
    try:
        dt = datetime.fromisoformat(ts_str.replace('Z', '+00:00'))
        return int(dt.timestamp())
    except Exception:
        return None


# ── Analysis Functions ───────────────────────────────────────────────────────

def analyze_trades(trades, label=""):
    """Comprehensive trade analysis returning a stats dict."""
    if not trades:
        return {"error": "No trades to analyze"}

    stats = {}
    stats['total_trades'] = len(trades)

    # Unique market windows
    slugs = [t.get('slug', '') for t in trades]
    unique_windows = set(slugs)
    stats['unique_windows'] = len(unique_windows)

    # By timeframe
    tf_counter = Counter()
    for t in trades:
        tf_counter[get_timeframe(t)] += 1
    stats['by_timeframe'] = dict(tf_counter.most_common())

    # By outcome (Up/Down)
    outcome_counter = Counter(t.get('outcome', '') for t in trades)
    stats['by_outcome'] = dict(outcome_counter)

    # By side (BUY/SELL)
    side_counter = Counter(t.get('side', '') for t in trades)
    stats['by_side'] = dict(side_counter)

    # Sizing
    sizes = []
    prices = []
    usdc_amounts = []
    for t in trades:
        size = float(t.get('size', 0) or 0)
        price = float(t.get('price', 0) or 0)
        if size > 0:
            sizes.append(size)
        if price > 0:
            prices.append(price)
        if size > 0 and price > 0:
            usdc_amounts.append(size * price)

    if sizes:
        sizes_sorted = sorted(sizes)
        stats['size_avg'] = sum(sizes) / len(sizes)
        stats['size_median'] = sizes_sorted[len(sizes_sorted) // 2]
        stats['size_min'] = min(sizes)
        stats['size_max'] = max(sizes)

    if prices:
        stats['price_avg'] = sum(prices) / len(prices)
        stats['price_min'] = min(prices)
        stats['price_max'] = max(prices)

    if usdc_amounts:
        stats['usdc_total'] = sum(usdc_amounts)
        stats['usdc_avg_per_trade'] = sum(usdc_amounts) / len(usdc_amounts)
        usdc_sorted = sorted(usdc_amounts)
        stats['usdc_median_per_trade'] = usdc_sorted[len(usdc_sorted) // 2]

    # Market window analysis
    window_trades = defaultdict(list)
    for t in trades:
        window_trades[t.get('slug', '')].append(t)

    # Both sides per window
    both_sides = 0
    up_only = 0
    down_only = 0
    for slug, wtrades in window_trades.items():
        outcomes = set(t.get('outcome', '') for t in wtrades)
        if 'Up' in outcomes and 'Down' in outcomes:
            both_sides += 1
        elif 'Up' in outcomes:
            up_only += 1
        elif 'Down' in outcomes:
            down_only += 1

    stats['windows_both_sides'] = both_sides
    stats['windows_up_only'] = up_only
    stats['windows_down_only'] = down_only

    # Capital per window
    cap_per_window = {}
    orders_per_window = {}
    for slug, wtrades in window_trades.items():
        cap = sum(float(t.get('size', 0) or 0) * float(t.get('price', 0) or 0) for t in wtrades)
        cap_per_window[slug] = cap
        orders_per_window[slug] = len(wtrades)

    caps = list(cap_per_window.values())
    if caps:
        stats['cap_per_window_avg'] = sum(caps) / len(caps)
        stats['cap_per_window_max'] = max(caps)
        stats['cap_per_window_min'] = min(caps)

    orders = list(orders_per_window.values())
    if orders:
        stats['orders_per_window_avg'] = sum(orders) / len(orders)
        stats['orders_per_window_median'] = sorted(orders)[len(orders) // 2]
        stats['orders_per_window_max'] = max(orders)

    # Top 10 windows by capital
    top_windows = sorted(cap_per_window.items(), key=lambda x: x[1], reverse=True)[:10]
    stats['top_windows'] = [
        {'slug': slug, 'capital': cap, 'trades': orders_per_window.get(slug, 0)}
        for slug, cap in top_windows
    ]

    # Trade timing analysis (how far into the window do trades land?)
    timing_buckets = {
        'before_open': 0,
        'early_0_25pct': 0,
        'mid_25_50pct': 0,
        'mid_50_75pct': 0,
        'late_75_100pct': 0,
        'after_close': 0,
    }
    timing_pcts = []  # % through window for each trade

    for t in trades:
        window_start = get_window_start_ts(t)
        trade_ts = get_trade_timestamp(t)
        if window_start is None or trade_ts is None:
            continue

        tf = get_timeframe(t)
        duration = {'5m': 300, '15m': 900, '1h': 3600, '4h': 14400}.get(tf, 0)
        if duration == 0:
            continue

        window_end = window_start + duration

        if trade_ts < window_start:
            timing_buckets['before_open'] += 1
        elif trade_ts >= window_end:
            timing_buckets['after_close'] += 1
        else:
            pct = (trade_ts - window_start) / duration
            timing_pcts.append(pct * 100)
            if pct < 0.25:
                timing_buckets['early_0_25pct'] += 1
            elif pct < 0.50:
                timing_buckets['mid_25_50pct'] += 1
            elif pct < 0.75:
                timing_buckets['mid_50_75pct'] += 1
            else:
                timing_buckets['late_75_100pct'] += 1

    stats['timing_buckets'] = timing_buckets
    if timing_pcts:
        sorted_pcts = sorted(timing_pcts)
        stats['entry_timing_avg_pct'] = sum(timing_pcts) / len(timing_pcts)
        stats['entry_timing_median_pct'] = sorted_pcts[len(sorted_pcts) // 2]
        stats['entry_timing_q1_pct'] = sorted_pcts[len(sorted_pcts) // 4]
        stats['entry_timing_q3_pct'] = sorted_pcts[3 * len(sorted_pcts) // 4]
        stats['entry_timing_min_pct'] = min(timing_pcts)
        stats['entry_timing_max_pct'] = max(timing_pcts)

    # Price distribution by outcome
    price_by_outcome = defaultdict(list)
    for t in trades:
        outcome = t.get('outcome', '')
        price = float(t.get('price', 0) or 0)
        if price > 0 and outcome:
            price_by_outcome[outcome].append(price)

    stats['price_distribution'] = {}
    for outcome, plist in price_by_outcome.items():
        if plist:
            stats['price_distribution'][outcome] = {
                'avg': sum(plist) / len(plist),
                'median': sorted(plist)[len(plist) // 2],
                'min': min(plist),
                'max': max(plist),
                'count': len(plist),
            }

    # Hourly distribution (UTC hour of trade)
    hour_counter = Counter()
    for t in trades:
        ts_str = t.get('timestamp', '')
        if ts_str:
            try:
                dt = datetime.fromisoformat(ts_str.replace('Z', '+00:00'))
                hour_counter[dt.hour] += 1
            except Exception:
                pass
    stats['by_hour_utc'] = dict(sorted(hour_counter.items()))

    # Daily distribution
    day_counter = Counter()
    day_usdc = defaultdict(float)
    for t in trades:
        ts_str = t.get('timestamp', '')
        if ts_str:
            try:
                dt = datetime.fromisoformat(ts_str.replace('Z', '+00:00'))
                day_str = dt.strftime('%Y-%m-%d')
                day_counter[day_str] += 1
                size = float(t.get('size', 0) or 0)
                price = float(t.get('price', 0) or 0)
                day_usdc[day_str] += size * price
            except Exception:
                pass
    stats['by_day'] = {d: {'trades': day_counter[d], 'usdc': day_usdc[d]} for d in sorted(day_counter.keys())}

    return stats


def analyze_window_detail(trades, top_n=5):
    """Detailed trade-by-trade analysis for top N windows by capital."""
    window_trades = defaultdict(list)
    for t in trades:
        window_trades[t.get('slug', '')].append(t)

    # Sort by capital
    window_caps = {}
    for slug, wtrades in window_trades.items():
        cap = sum(float(t.get('size', 0) or 0) * float(t.get('price', 0) or 0) for t in wtrades)
        window_caps[slug] = cap

    top_slugs = sorted(window_caps, key=window_caps.get, reverse=True)[:top_n]

    details = []
    for slug in top_slugs:
        wtrades = window_trades[slug]
        window_start = None
        tf = 'unknown'
        parts = slug.split('-')
        for p in parts:
            try:
                ts = int(p)
                if ts > 1700000000:
                    window_start = ts
            except ValueError:
                pass
            if p in ('5m', '15m', '1h', '4h'):
                tf = p

        duration = {'5m': 300, '15m': 900, '1h': 3600, '4h': 14400}.get(tf, 0)
        window_end = window_start + duration if window_start and duration else None

        window_detail = {
            'slug': slug,
            'timeframe': tf,
            'window_open': datetime.fromtimestamp(window_start, tz=timezone.utc).isoformat() if window_start else 'unknown',
            'window_close': datetime.fromtimestamp(window_end, tz=timezone.utc).isoformat() if window_end else 'unknown',
            'total_capital': window_caps[slug],
            'num_trades': len(wtrades),
            'trades': [],
        }

        for t in sorted(wtrades, key=lambda x: x.get('timestamp', '')):
            trade_ts = get_trade_timestamp(t)
            timing = ''
            if window_start and trade_ts and duration:
                if trade_ts < window_start:
                    timing = f"{window_start - trade_ts}s before open"
                elif trade_ts >= window_end:
                    timing = f"+{trade_ts - window_end}s after close"
                else:
                    pct = (trade_ts - window_start) / duration * 100
                    timing = f"{pct:.0f}% through window"

            size = float(t.get('size', 0) or 0)
            price = float(t.get('price', 0) or 0)

            window_detail['trades'].append({
                'timestamp': t.get('timestamp', ''),
                'outcome': t.get('outcome', ''),
                'side': t.get('side', ''),
                'size': size,
                'price': price,
                'usdc': size * price,
                'timing': timing,
            })

        details.append(window_detail)

    return details


# ── Report Generator ─────────────────────────────────────────────────────────

def generate_report(stats_5m, stats_15m, stats_combined, wallet360_data,
                    pnl_data, window_details, wallet, days, report_time):
    """Generate markdown report."""
    lines = []
    w = lines.append

    w(f"# BTC 5m/15m Trader Report — Idolized-Scallops")
    w(f"")
    w(f"**Generated:** {report_time.strftime('%Y-%m-%d %H:%M UTC')}")
    w(f"**Wallet:** `{wallet}`")
    w(f"**Period:** Last {days} days ({(report_time - timedelta(days=days)).strftime('%Y-%m-%d')} to {report_time.strftime('%Y-%m-%d')})")
    w(f"**Filter:** BTC Up/Down markets, 5m and 15m timeframes only")
    w(f"")
    w(f"---")
    w(f"")

    # ── Executive Summary ────────────────────────────────────────────────
    w(f"## 1. Executive Summary")
    w(f"")
    total = stats_combined.get('total_trades', 0)
    usdc = stats_combined.get('usdc_total', 0)
    windows = stats_combined.get('unique_windows', 0)
    both = stats_combined.get('windows_both_sides', 0)
    w(f"- **{total:,} trades** across **{windows} market windows** on BTC 5m/15m")
    w(f"- **${usdc:,.0f}** total USDC deployed")
    if windows > 0:
        w(f"- **{both}/{windows}** windows ({both/windows*100:.1f}%) traded both Up+Down sides")
    timing = stats_combined.get('timing_buckets', {})
    after_close = timing.get('after_close', 0)
    total_timed = sum(timing.values())
    if total_timed > 0:
        w(f"- **{after_close}/{total_timed}** trades ({after_close/total_timed*100:.1f}%) executed after window close")
    if stats_combined.get('entry_timing_avg_pct'):
        w(f"- Avg entry at **{stats_combined['entry_timing_avg_pct']:.0f}%** through window (median {stats_combined.get('entry_timing_median_pct', 0):.0f}%)")
    w(f"")

    # 5m vs 15m split
    w(f"### 5m vs 15m Breakdown")
    w(f"")
    w(f"| Metric | 5m | 15m |")
    w(f"|---|---|---|")
    w(f"| Trades | {stats_5m.get('total_trades', 0):,} | {stats_15m.get('total_trades', 0):,} |")
    w(f"| Windows | {stats_5m.get('unique_windows', 0)} | {stats_15m.get('unique_windows', 0)} |")
    w(f"| USDC Deployed | ${stats_5m.get('usdc_total', 0):,.0f} | ${stats_15m.get('usdc_total', 0):,.0f} |")
    w(f"| Avg USDC/Trade | ${stats_5m.get('usdc_avg_per_trade', 0):,.2f} | ${stats_15m.get('usdc_avg_per_trade', 0):,.2f} |")
    w(f"| Avg Price Paid | ${stats_5m.get('price_avg', 0):.4f} | ${stats_15m.get('price_avg', 0):.4f} |")
    w(f"| Both Sides Windows | {stats_5m.get('windows_both_sides', 0)} | {stats_15m.get('windows_both_sides', 0)} |")
    cap_5m = stats_5m.get('cap_per_window_avg', 0)
    cap_15m = stats_15m.get('cap_per_window_avg', 0)
    w(f"| Avg Capital/Window | ${cap_5m:,.0f} | ${cap_15m:,.0f} |")
    w(f"")
    w(f"---")
    w(f"")

    # ── Trade Timing ─────────────────────────────────────────────────────
    w(f"## 2. Trade Timing Analysis")
    w(f"")
    w(f"### When in the window do trades execute?")
    w(f"")
    w(f"| Timing Bucket | Count | % |")
    w(f"|---|---|---|")
    for bucket, count in stats_combined.get('timing_buckets', {}).items():
        label = bucket.replace('_', ' ').title()
        pct = count / max(total_timed, 1) * 100
        w(f"| {label} | {count:,} | {pct:.1f}% |")
    w(f"")

    if stats_combined.get('entry_timing_avg_pct') is not None:
        w(f"### Entry Timing Stats (% through window)")
        w(f"")
        w(f"| Metric | Value |")
        w(f"|---|---|")
        w(f"| Avg entry point | {stats_combined.get('entry_timing_avg_pct', 0):.1f}% |")
        w(f"| Median entry point | {stats_combined.get('entry_timing_median_pct', 0):.1f}% |")
        w(f"| Q1 (25th percentile) | {stats_combined.get('entry_timing_q1_pct', 0):.1f}% |")
        w(f"| Q3 (75th percentile) | {stats_combined.get('entry_timing_q3_pct', 0):.1f}% |")
        w(f"| Earliest entry | {stats_combined.get('entry_timing_min_pct', 0):.1f}% |")
        w(f"| Latest entry | {stats_combined.get('entry_timing_max_pct', 0):.1f}% |")
        w(f"")
    w(f"---")
    w(f"")

    # ── Position Sizing ──────────────────────────────────────────────────
    w(f"## 3. Position Sizing")
    w(f"")
    w(f"| Metric | Value |")
    w(f"|---|---|")
    w(f"| Avg trade size (shares) | {stats_combined.get('size_avg', 0):.2f} |")
    w(f"| Median trade size | {stats_combined.get('size_median', 0):.2f} |")
    w(f"| Min / Max | {stats_combined.get('size_min', 0):.2f} / {stats_combined.get('size_max', 0):.2f} |")
    w(f"| Avg USDC per trade | ${stats_combined.get('usdc_avg_per_trade', 0):.2f} |")
    w(f"| Median USDC per trade | ${stats_combined.get('usdc_median_per_trade', 0):.2f} |")
    w(f"| Total USDC deployed | ${stats_combined.get('usdc_total', 0):,.0f} |")
    w(f"")

    w(f"### Capital Per Market Window")
    w(f"")
    w(f"| Metric | Value |")
    w(f"|---|---|")
    w(f"| Avg capital/window | ${stats_combined.get('cap_per_window_avg', 0):,.0f} |")
    w(f"| Max capital in single window | ${stats_combined.get('cap_per_window_max', 0):,.0f} |")
    w(f"| Min capital | ${stats_combined.get('cap_per_window_min', 0):,.0f} |")
    w(f"| Avg orders/window | {stats_combined.get('orders_per_window_avg', 0):.1f} |")
    w(f"| Max orders in single window | {stats_combined.get('orders_per_window_max', 0)} |")
    w(f"")
    w(f"---")
    w(f"")

    # ── Outcome & Side Distribution ──────────────────────────────────────
    w(f"## 4. Outcome & Side Distribution")
    w(f"")
    w(f"### By Outcome")
    w(f"")
    w(f"| Outcome | Count | % |")
    w(f"|---|---|---|")
    for outcome, count in stats_combined.get('by_outcome', {}).items():
        pct = count / max(total, 1) * 100
        w(f"| {outcome} | {count:,} | {pct:.1f}% |")
    w(f"")

    w(f"### By Side (BUY/SELL)")
    w(f"")
    w(f"| Side | Count | % |")
    w(f"|---|---|---|")
    for side, count in stats_combined.get('by_side', {}).items():
        pct = count / max(total, 1) * 100
        w(f"| {side} | {count:,} | {pct:.1f}% |")
    w(f"")

    w(f"### Price Distribution by Outcome")
    w(f"")
    w(f"| Outcome | Count | Avg Price | Median | Min | Max |")
    w(f"|---|---|---|---|---|---|")
    for outcome, pdist in stats_combined.get('price_distribution', {}).items():
        w(f"| {outcome} | {pdist['count']:,} | ${pdist['avg']:.4f} | ${pdist['median']:.4f} | ${pdist['min']:.4f} | ${pdist['max']:.4f} |")
    w(f"")

    w(f"### Market Window Sides")
    w(f"")
    w(f"| Type | Count | % |")
    w(f"|---|---|---|")
    ws_total = stats_combined.get('windows_both_sides', 0) + stats_combined.get('windows_up_only', 0) + stats_combined.get('windows_down_only', 0)
    for label, key in [('Both Up+Down', 'windows_both_sides'), ('Up only', 'windows_up_only'), ('Down only', 'windows_down_only')]:
        val = stats_combined.get(key, 0)
        pct = val / max(ws_total, 1) * 100
        w(f"| {label} | {val} | {pct:.1f}% |")
    w(f"")
    w(f"---")
    w(f"")

    # ── Daily Breakdown ──────────────────────────────────────────────────
    w(f"## 5. Daily Breakdown")
    w(f"")
    w(f"| Date | Trades | USDC Deployed |")
    w(f"|---|---|---|")
    for day, dstats in stats_combined.get('by_day', {}).items():
        w(f"| {day} | {dstats['trades']:,} | ${dstats['usdc']:,.0f} |")
    w(f"")
    w(f"---")
    w(f"")

    # ── Hourly Distribution ──────────────────────────────────────────────
    w(f"## 6. Hourly Activity Distribution (UTC)")
    w(f"")
    w(f"| Hour | Trades | % |")
    w(f"|---|---|---|")
    for hour, count in stats_combined.get('by_hour_utc', {}).items():
        pct = count / max(total, 1) * 100
        bar = '#' * int(pct / 2)
        w(f"| {hour:02d}:00 | {count:,} | {pct:.1f}% {bar} |")
    w(f"")
    w(f"---")
    w(f"")

    # ── Top Windows Detail ───────────────────────────────────────────────
    w(f"## 7. Top Market Windows by Capital")
    w(f"")
    w(f"| # | Slug | Capital | Trades |")
    w(f"|---|---|---|---|")
    for i, tw in enumerate(stats_combined.get('top_windows', []), 1):
        w(f"| {i} | `{tw['slug']}` | ${tw['capital']:,.0f} | {tw['trades']} |")
    w(f"")

    # Detailed window trade-by-trade
    if window_details:
        w(f"### Detailed Trade-by-Trade (Top 5 Windows)")
        w(f"")
        for wd in window_details[:5]:
            w(f"#### `{wd['slug']}` ({wd['timeframe']}, {wd.get('window_open', 'unknown')} → {wd.get('window_close', 'unknown')})")
            w(f"Capital: ${wd['total_capital']:,.0f} | Trades: {wd['num_trades']}")
            w(f"")
            w(f"| Time | Outcome | Side | Size | Price | USDC | Timing |")
            w(f"|---|---|---|---|---|---|---|")
            for tr in wd['trades'][:30]:  # limit to 30 per window
                w(f"| {tr['timestamp'][-12:]} | {tr['outcome']} | {tr['side']} | {tr['size']:.1f} | ${tr['price']:.4f} | ${tr['usdc']:.2f} | {tr['timing']} |")
            if len(wd['trades']) > 30:
                w(f"| ... | *{len(wd['trades']) - 30} more trades* | | | | | |")
            w(f"")
    w(f"---")
    w(f"")

    # ── Wallet 360 ───────────────────────────────────────────────────────
    w(f"## 8. Wallet 360 Metrics (All Markets)")
    w(f"")
    if wallet360_data:
        w(f"| Window | Trades | Invested | PnL | ROI | PF | W/L | Sharpe |")
        w(f"|---|---|---|---|---|---|---|---|")
        for window_label, w360 in wallet360_data.items():
            if w360:
                w(f"| {window_label} | {w360.get('total_trades', 'N/A'):,} | ${float(w360.get('total_invested', 0)):,.0f} | ${float(w360.get('total_pnl', 0)):,.0f} | {w360.get('roi', 'N/A')}% | {w360.get('profit_factor', 'N/A')} | {w360.get('winning_trades', '?')}/{w360.get('losing_trades', '?')} | {w360.get('sharpe_ratio', 'N/A')} |")
        w(f"")

        # Latest window details
        latest = list(wallet360_data.values())[-1]
        if latest:
            w(f"### Additional Metrics (latest window)")
            w(f"")
            w(f"| Metric | Value |")
            w(f"|---|---|")
            for key in ['equity_curve_pattern', 'risk_level', 'best_trade', 'worst_trade',
                        'perfect_timing_score', 'perfect_entry_count', 'sybil_cluster_size',
                        'num_proxy_wallets', 'flagged_metrics', 'markets_traded', 'avg_position_size']:
                val = latest.get(key)
                if val is not None:
                    label = key.replace('_', ' ').title()
                    if 'trade' in key or 'position' in key or 'invested' in key:
                        try:
                            w(f"| {label} | ${float(val):,.2f} |")
                        except (ValueError, TypeError):
                            w(f"| {label} | {val} |")
                    else:
                        w(f"| {label} | {val} |")
            w(f"")

            # Decode performance_by_category
            perf_b64 = latest.get('performance_by_category', '')
            if perf_b64:
                try:
                    perf = json.loads(base64.b64decode(perf_b64))
                    w(f"### Performance by Category")
                    w(f"")
                    w(f"```json")
                    w(json.dumps(perf, indent=2))
                    w(f"```")
                    w(f"")
                except Exception:
                    pass
    else:
        w(f"*No Wallet 360 data available*")
    w(f"")
    w(f"---")
    w(f"")

    # ── PnL ──────────────────────────────────────────────────────────────
    w(f"## 9. Daily PnL (All Markets)")
    w(f"")
    if pnl_data:
        w(f"| Date | Trades | Invested | PnL | ROI | Wins | Losses |")
        w(f"|---|---|---|---|---|---|---|")
        for entry in pnl_data:
            date = entry.get('date', entry.get('period', ''))
            trades_count = entry.get('total_trades', entry.get('trades', ''))
            invested = float(entry.get('total_invested', entry.get('invested', 0)) or 0)
            pnl = float(entry.get('total_pnl', entry.get('pnl', 0)) or 0)
            roi = f"{pnl / invested * 100:.2f}" if invested > 0 else "N/A"
            wins = entry.get('winning_trades', entry.get('wins', ''))
            losses = entry.get('losing_trades', entry.get('losses', ''))
            w(f"| {date} | {trades_count} | ${invested:,.0f} | ${pnl:+,.0f} | {roi}% | {wins} | {losses} |")
        w(f"")
    else:
        w(f"*No PnL data available for this period*")
    w(f"")
    w(f"---")
    w(f"")

    # ── Key Insights ─────────────────────────────────────────────────────
    w(f"## 10. Key Insights & Strategy Characteristics")
    w(f"")

    # Auto-generate insights from data
    after_pct = after_close / max(total_timed, 1) * 100
    both_pct = both / max(ws_total, 1) * 100

    during_pct = 100 - after_pct
    avg_entry = stats_combined.get('entry_timing_avg_pct', 50)
    med_entry = stats_combined.get('entry_timing_median_pct', 50)

    if during_pct > 90:
        if avg_entry > 50:
            w(f"1. **Active In-Window Momentum Strategy**: {during_pct:.1f}% of trades during live window, avg entry at {avg_entry:.0f}% (median {med_entry:.0f}%). Waits for price confirmation before committing.")
        else:
            w(f"1. **Active In-Window Early Entry**: {during_pct:.1f}% of trades during live window, avg entry at {avg_entry:.0f}% (median {med_entry:.0f}%). Takes positions early in the window.")
    elif during_pct > 50:
        w(f"1. **Hybrid Strategy**: {during_pct:.1f}% during window, {after_pct:.1f}% post-close — mixes directional and settlement plays.")
    else:
        w(f"1. **Post-Close Settlement Strategy**: {after_pct:.1f}% of trades execute after window closes — settlement arb, not prediction.")

    if both_pct > 80:
        w(f"2. **Non-Directional**: {both_pct:.1f}% of windows have both Up+Down trades — classic market making / arb behavior.")
    elif both_pct > 50:
        w(f"2. **Mixed Directional**: {both_pct:.1f}% of windows have both sides — partially directional, partially hedged.")
    else:
        w(f"2. **Directional**: Only {both_pct:.1f}% of windows trade both sides — this trader takes directional bets.")

    tf_data = stats_combined.get('by_timeframe', {})
    dominant_tf = max(tf_data, key=tf_data.get) if tf_data else 'unknown'
    w(f"3. **Dominant Timeframe**: {dominant_tf} with {tf_data.get(dominant_tf, 0):,} trades ({tf_data.get(dominant_tf, 0)/max(total, 1)*100:.1f}%)")

    if stats_combined.get('usdc_total', 0) > 0 and stats_combined.get('unique_windows', 0) > 0:
        avg_cap = stats_combined['usdc_total'] / stats_combined['unique_windows']
        w(f"4. **Avg Capital Per Window**: ${avg_cap:,.0f} — {'aggressive' if avg_cap > 5000 else 'moderate' if avg_cap > 1000 else 'conservative'} sizing")

    if stats_combined.get('orders_per_window_avg', 0) > 20:
        w(f"5. **High Order Density**: {stats_combined['orders_per_window_avg']:.0f} orders/window avg — automated book sweeping, not single fills")
    elif stats_combined.get('orders_per_window_avg', 0) > 5:
        w(f"5. **Moderate Order Density**: {stats_combined['orders_per_window_avg']:.0f} orders/window — multiple fills per entry")
    else:
        w(f"5. **Low Order Density**: {stats_combined['orders_per_window_avg']:.0f} orders/window — targeted entries")

    daily = stats_combined.get('by_day', {})
    if daily:
        daily_trades = [(d, s['trades']) for d, s in daily.items()]
        if daily_trades:
            best_day = max(daily_trades, key=lambda x: x[1])
            worst_day = min(daily_trades, key=lambda x: x[1])
            w(f"6. **Most Active Day**: {best_day[0]} ({best_day[1]:,} BTC trades)")
            w(f"7. **Least Active Day**: {worst_day[0]} ({worst_day[1]:,} BTC trades)")

    w(f"")
    w(f"---")
    w(f"")
    w(f"## Appendix: Raw Data")
    w(f"")
    w(f"Raw JSON data saved alongside this report for further analysis.")
    w(f"")

    return "\n".join(lines)


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="BTC 5m/15m Trader Report Generator")
    parser.add_argument("--days", type=int, default=3, help="Number of days to analyze (default: 3)")
    parser.add_argument("--wallet", default=DEFAULT_WALLET, help="Wallet address to analyze")
    parser.add_argument("--api-key", default=DEFAULT_API_KEY, help="Heisenberg API key")
    parser.add_argument("--output", default=None, help="Output file path (default: auto-generated)")
    args = parser.parse_args()

    now = datetime.now(timezone.utc)
    start = now - timedelta(days=args.days)
    start_unix = int(start.timestamp())
    end_unix = int(now.timestamp())

    headers = make_headers(args.api_key)

    print(f"=" * 70)
    print(f"BTC 5m/15m Trader Report Generator")
    print(f"Wallet: {args.wallet}")
    print(f"Period: {start.strftime('%Y-%m-%d %H:%M')} to {now.strftime('%Y-%m-%d %H:%M')} UTC ({args.days} days)")
    print(f"=" * 70)

    # ── Step 1: Fetch all trades ─────────────────────────────────────────
    print(f"\n[1/4] Fetching all trades ({args.days} days)...")
    all_trades = fetch_all_trades(args.wallet, start_unix, end_unix, headers)
    print(f"  Total trades fetched: {len(all_trades)}")

    # ── Step 2: Filter to BTC 5m/15m ─────────────────────────────────────
    print(f"\n[2/4] Filtering to BTC 5m/15m...")
    btc_trades = [t for t in all_trades if is_btc_trade(t)]
    btc_5m_15m = [t for t in btc_trades if is_5m_or_15m(t)]
    btc_5m = [t for t in btc_5m_15m if get_timeframe(t) == '5m']
    btc_15m = [t for t in btc_5m_15m if get_timeframe(t) == '15m']

    print(f"  All trades: {len(all_trades)}")
    print(f"  BTC trades: {len(btc_trades)}")
    print(f"  BTC 5m/15m: {len(btc_5m_15m)}")
    print(f"  BTC 5m: {len(btc_5m)}")
    print(f"  BTC 15m: {len(btc_15m)}")

    # Show a sample trade for debugging
    if btc_5m_15m:
        print(f"\n  Sample trade:")
        print(f"  {json.dumps(btc_5m_15m[0], indent=2, default=str)[:500]}")

    # ── Step 3: Analyze ──────────────────────────────────────────────────
    print(f"\n[3/4] Analyzing...")
    stats_5m = analyze_trades(btc_5m, "5m")
    stats_15m = analyze_trades(btc_15m, "15m")
    stats_combined = analyze_trades(btc_5m_15m, "combined")
    window_details = analyze_window_detail(btc_5m_15m, top_n=5)

    # ── Step 4: Fetch supplementary data ─────────────────────────────────
    print(f"\n[4/4] Fetching supplementary data...")

    # Wallet 360 for multiple windows
    wallet360_data = {}
    for window in ["1", "3", "7"]:
        print(f"  Wallet 360 ({window}d)...")
        w360 = fetch_wallet360(args.wallet, window, headers)
        wallet360_data[f"{window}d"] = w360
        time.sleep(0.5)

    # PnL for the period
    start_str = start.strftime('%Y-%m-%d')
    end_str = now.strftime('%Y-%m-%d')
    print(f"  PnL ({start_str} to {end_str})...")
    pnl_data = fetch_pnl(args.wallet, start_str, end_str, "1d", headers)

    # ── Generate report ──────────────────────────────────────────────────
    print(f"\nGenerating report...")
    report = generate_report(
        stats_5m, stats_15m, stats_combined,
        wallet360_data, pnl_data, window_details,
        args.wallet, args.days, now,
    )

    # Save outputs
    output_dir = Path("/Users/milerius/Documents/Mantis/polymarket/docs/research")
    output_dir.mkdir(parents=True, exist_ok=True)

    date_str = now.strftime('%Y-%m-%d')
    if args.output:
        report_path = Path(args.output)
    else:
        report_path = output_dir / f"{date_str}-btc-5m-15m-report.md"

    raw_data_path = output_dir / f"{date_str}-btc-5m-15m-raw.json"

    with open(report_path, 'w') as f:
        f.write(report)
    print(f"Report saved: {report_path}")

    raw_data = {
        'metadata': {
            'wallet': args.wallet,
            'days': args.days,
            'generated': now.isoformat(),
            'start': start.isoformat(),
            'end': now.isoformat(),
        },
        'stats_5m': stats_5m,
        'stats_15m': stats_15m,
        'stats_combined': stats_combined,
        'wallet360': wallet360_data,
        'pnl': pnl_data,
        'window_details': window_details,
        'all_btc_5m_15m_trades': btc_5m_15m,
    }

    with open(raw_data_path, 'w') as f:
        json.dump(raw_data, f, indent=2, default=str)
    print(f"Raw data saved: {raw_data_path}")

    print(f"\n{'=' * 70}")
    print(f"REPORT COMPLETE")
    print(f"{'=' * 70}")

    # Print quick summary to stdout
    print(f"\nQuick Summary:")
    print(f"  BTC 5m/15m trades (last {args.days}d): {len(btc_5m_15m):,}")
    print(f"  USDC deployed: ${stats_combined.get('usdc_total', 0):,.0f}")
    print(f"  Unique windows: {stats_combined.get('unique_windows', 0)}")
    timing = stats_combined.get('timing_buckets', {})
    total_timed = sum(timing.values())
    if total_timed > 0:
        after_pct = timing.get('after_close', 0) / total_timed * 100
        print(f"  Post-close trades: {after_pct:.1f}%")
    print(f"  Report: {report_path}")


if __name__ == "__main__":
    main()
