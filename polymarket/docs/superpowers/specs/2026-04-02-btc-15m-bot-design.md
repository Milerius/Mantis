# BTC 15m Up/Down Trading Bot — Design Spec

**Date:** 2026-04-02
**Status:** Approved
**Implementation:** Single Python file (`scripts/btc_15m_bot.py`)
**Modes:** Paper (orderbook simulation) | Micro-Live (real $1 orders) | Live (full size)

---

## 1. Goal

Build a Python proof-of-concept bot that trades Polymarket BTC 15-minute Up/Down markets using a GTC limit order strategy derived from — and improved upon — the Idolized-Scallops trader analysis.

The bot must:
- Only enter at window open (never mid-window)
- Use Binance BTC spot price as the directional signal
- Place GTC limit orders on the favored side at $0.45-$0.70
- Place penny insurance on the opposite side at $0.01-$0.08
- Record every window for post-session replay
- Optionally spy on a target wallet for comparison

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    CONFIG (dict at top)                  │
│  Strategy params, API keys, mode (paper/micro-live/live)│
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│                  MAIN LOOP (single thread)               │
│                                                          │
│  WindowManager     — compute next window, wait for T+0   │
│  MarketDiscovery   — find active BTC 15m via Gamma API   │
│  SignalEngine      — Binance WS, direction at T+15s      │
│  OrderManager      — post GTC ladder + insurance         │
│  PositionTracker   — track fills, enforce hard caps      │
│  SettlementHandler — resolve, compute P&L, write replay  │
└──────────────────────┬──────────────────────────────────┘
                       │ reads (optional, non-blocking)
                  ┌────▼────┐
                  │ spy_data│ (thread-safe dict)
                  └────▲────┘
                       │ writes
┌──────────────────────┴──────────────────────────────────┐
│             SPY THREAD (daemon, optional)                 │
│  Polls target wallet trades via Heisenberg API every 5s  │
│  Populates spy_data with direction, fills, position      │
│  Read-only observation — does NOT influence trading       │
└─────────────────────────────────────────────────────────┘
```

---

## 3. Window Lifecycle

```
T-5s     WindowManager wakes up, discovers market via Gamma API
T+0s     Window opens. Binance price recorded as open_price.
T+15s    SignalEngine reads Binance spot, computes delta.
         If |delta| < min_btc_delta: skip this window.
         Otherwise: direction = Up if delta > 0, Down if delta < 0.
T+16s    OrderManager posts GTC ladder on favored side.
T+50%    OrderManager posts insurance orders on opposite side.
T+80%    All unfilled GTC orders cancelled. No new orders.
T+900s   Window closes. Wait for resolution.
T+~910s  SettlementHandler fetches outcome, computes final P&L.
T+~915s  WindowRecorder writes replay JSON. Loop to next window.
```

If a window is already in progress when the bot starts, it waits for the next fresh window.

---

## 4. Component Details

### 4.1 WindowManager

- Computes next window: `next_open = ((now // 900) + 1) * 900`
- Sleeps until `next_open - 5s` to allow market discovery
- Provides `window_open`, `window_close`, `pct_through()` helper
- Skips to next window if current one is already >5% through at boot

### 4.2 MarketDiscovery

- Queries Gamma API: `GET /events?active=true&closed=false&tag_slug=up-or-down&tag_slug=15M`
- Filters for BTC, extracts `condition_id`, `token_up`, `token_down`, `end_date`
- Matches window time to market slug timestamp
- Caches results (markets don't change within a window)

### 4.3 SignalEngine

- Maintains Binance WebSocket connection: `wss://stream.binance.com:9443/ws/btcusdt@trade`
- Records `open_price` at T+0 (first tick after window open)
- At T+signal_delay (15s), reads current price
- `delta = current - open_price`
- If `abs(delta) < min_btc_delta_usd`: skip window (no clear signal)
- Direction: `"Up" if delta > 0 else "Down"`

### 4.4 OrderManager

Posts GTC orders through the Executor abstraction:

**Phase 1 — Favored side (T+16s):**
- For each price in `favored_prices` where price <= `favored_max_price`:
  - `executor.place_gtc_order(favored_token, BUY, price, shares)`
- Shares = `favored_shares` (or `micro_live_size` in micro-live mode)

**Phase 2 — Insurance (T+50%):**
- For each price in `insurance_prices` where price <= `insurance_max_price`:
  - `executor.place_gtc_order(opposite_token, BUY, price, shares)`

**Phase 3 — Cleanup (T+80%):**
- `executor.cancel_all()`
- No new orders after this point

Side switches: if direction reverses (rare), OrderManager can cancel favored orders and re-post on the new side, up to `max_side_switches` times.

### 4.5 PositionTracker

- Polls fills every 2 seconds (paper mode) or receives via User WebSocket (live)
- Maintains cumulative: `up_shares`, `up_cost`, `down_shares`, `down_cost`
- Enforces hard caps before every order:
  - `total_deployed < max_deploy_per_window`
  - `price <= max_price` for the order type
  - `daily_loss < max_daily_loss`
- Provides live P&L estimate: `pnl_if_up()` and `pnl_if_down()`

### 4.6 SettlementHandler

- After window close, polls Gamma API for resolution (Up or Down)
- Computes final P&L: `winner_shares - total_deployed`
- Updates daily P&L tracker
- Triggers WindowRecorder

### 4.7 WindowRecorder

Writes one JSON file per window to `replay_dir/`:

```
window_replay/2026-04-02_14-30_btc-15m.json
```

Contents:
```json
{
  "window": {
    "slug": "btc-updown-15m-1775140200",
    "open_time": "2026-04-02T14:30:00Z",
    "close_time": "2026-04-02T14:45:00Z",
    "winner": "Up"
  },
  "signal": {
    "btc_open_price": 84230.50,
    "btc_at_signal": 84285.10,
    "delta": 54.60,
    "direction": "Up",
    "signal_time": "2026-04-02T14:30:15Z"
  },
  "our_trades": [...],
  "our_position": {
    "up_shares": 420, "up_cost": 238,
    "down_shares": 180, "down_cost": 12,
    "total_deployed": 250,
    "pnl": 170, "roi_pct": 68.0
  },
  "spy": {
    "wallet": "0xe1d6...",
    "trades": [...],
    "position": {...},
    "direction": "Down",
    "w_avg": 0.661
  },
  "comparison": {
    "our_pnl": 170,
    "spy_pnl": 370,
    "same_direction": false,
    "our_w_avg": 0.567,
    "spy_w_avg": 0.661
  }
}
```

### 4.8 SpyThread (optional)

- Daemon thread, started if `spy_enabled=True`
- Polls Heisenberg API (agent 556) every `spy_poll_interval_sec` for target wallet trades in current window's time range
- Updates thread-safe `spy_data` dict with: direction, fills, cumulative position
- At window close, does one final fetch for complete data
- Main thread reads `spy_data` for the replay file but never blocks on it

---

## 5. Execution Layer

Three modes sharing the same interface:

```python
class Executor:
    place_gtc_order(token_id, side, price, shares) → order_id
    cancel_order(order_id)
    cancel_all()
    get_fills() → list[(order_id, price, shares, timestamp)]
    get_open_orders() → list[order_id]
```

### 5.1 PaperExecutor

- On `place_gtc_order`: fetches live orderbook from `GET https://clob.polymarket.com/book?token_id=XXX`
- If `price >= best_ask`: immediate fill at ask price (simulates crossing the spread)
- Otherwise: stores as resting order, re-checks every tick (2s) against fresh orderbook
- Partial fills if ask size < our order size
- Tracks all fills with real timestamps
- `cancel_order`: removes from resting list

### 5.2 MicroLiveExecutor

- Wraps `py-clob-client` with enforced tiny sizes
- On `place_gtc_order`: overrides `shares = min(shares, micro_live_size)`
- Posts real GTC order via `client.create_order()` + `client.post_order()`
- `get_fills`: polls `client.get_order(order_id)` for `size_matched`
- `cancel_order`: calls `client.cancel(order_id)`
- Requires `POLYMARKET_PRIVATE_KEY` env var

### 5.3 LiveExecutor

- Same as MicroLiveExecutor but uses config's `favored_shares` / `insurance_shares`
- Uses User WebSocket for real-time fill tracking
- All safety checks enforced at this level (max price, max deploy)

---

## 6. Config

```python
CONFIG = {
    # Mode
    "mode": "paper",              # "paper" | "micro-live" | "live"
    "micro_live_size": 1.0,       # shares per level in micro-live

    # Market
    "asset": "btc",
    "timeframe": "15m",
    "window_duration": 900,

    # Signal
    "signal_delay_sec": 15,
    "min_btc_delta_usd": 10.0,

    # Favored Side
    "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
    "favored_shares": 100,
    "favored_max_price": 0.75,

    # Insurance
    "insurance_prices": [0.01, 0.02, 0.03, 0.05, 0.08],
    "insurance_shares": 100,
    "insurance_max_price": 0.10,
    "insurance_start_pct": 50,

    # Timing
    "stop_trading_pct": 80,
    "max_side_switches": 3,

    # Risk
    "max_deploy_per_window": 500,
    "max_daily_loss": 1000,
    "max_consecutive_losses": 8,

    # Spy
    "spy_enabled": False,
    "spy_wallet": "0xe1d6b51521bd4365769199f392f9818661bd907c",
    "spy_poll_interval_sec": 5,

    # Recording
    "replay_dir": "window_replay/",
    "log_file": "bot.log",
}
```

All strategy parameters are tunable without code changes. The quant simulation (`scripts/strategy_simulation.py` and `scripts/multi_config_simulation.py`) can test different configs before deploying.

---

## 7. Safety Rules

**Hard rules checked before every order (enforced at Executor level):**

| Rule | Check | Action |
|---|---|---|
| Price cap (favored) | `price <= favored_max_price` | Reject order |
| Price cap (insurance) | `price <= insurance_max_price` | Reject order |
| Window budget | `total_deployed <= max_deploy_per_window` | Reject order |
| Daily loss limit | `daily_loss <= max_daily_loss` | Halt all trading |
| Time cutoff | `window_pct <= stop_trading_pct` | Cancel all, no new orders |
| Side switch limit | `switches <= max_side_switches` | Ignore direction change |
| Micro-live cap | `shares <= micro_live_size` (in micro-live mode) | Override shares |

**Invariants (assert, crash on violation):**
- Never place order without valid token_id
- Never place order with size <= 0 or price <= 0
- Never place order after window close time
- Never start trading a window that's already >5% through

---

## 8. Dependencies

```
py-clob-client          # Polymarket CLOB SDK (GTC orders, orderbook)
websockets              # Binance WebSocket + Polymarket WS
requests                # Gamma API, Heisenberg API, CLOB REST
```

No heavy dependencies. Standard library for everything else (threading, json, time, dataclasses, argparse, logging).

---

## 9. Usage

```bash
# Paper mode (default) — simulates against real orderbook
python scripts/btc_15m_bot.py

# Paper mode with spy
python scripts/btc_15m_bot.py --spy 0xe1d6b51521bd4365769199f392f9818661bd907c

# Micro-live ($1 real orders)
POLYMARKET_PRIVATE_KEY=0x... python scripts/btc_15m_bot.py --mode micro-live

# Full live
POLYMARKET_PRIVATE_KEY=0x... python scripts/btc_15m_bot.py --mode live
```

---

## 10. Quantitative Basis

Strategy parameters validated via simulation across 41 real Scallops windows:

| Metric | Value |
|---|---|
| Breakeven accuracy | 37-43% (depending on config) |
| Theoretical win PnL | +$210 to +$236 |
| Theoretical loss PnL | -$160 to +$136 (insurance can flip losses to profits) |
| Win/Loss ratio | 1.31x (conservative) to 1.74x (heavy insurance) |
| $0.75 cap rule | 0% win rate above $0.75 in 20 measurable windows |
| Recovery when wrong | 59-137% of deployed capital |

Full simulation code: `scripts/strategy_simulation.py`, `scripts/multi_config_simulation.py`, `scripts/scenario_analysis.py`

---

## 11. Future Extensions (not in v1)

- Multi-asset support (ETH, SOL, XRP)
- 5-minute window support
- Spy signal as optional confirmation input
- Auto-tuning of price ladder based on recent fill rates
- Dashboard / web UI for monitoring
