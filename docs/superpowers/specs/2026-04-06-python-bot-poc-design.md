# Python Bot POC — Full-Stack Design Spec

**Date:** 2026-04-06
**Status:** Approved
**Scope:** `polymarket-bot-python-poc/` — complete trading bot proving the Rust architecture

---

## 1. Goals

- Full-stack Polymarket trading bot in Python as a proving ground for the Rust SDK architecture
- Same logical architecture as the brainstormed 8-thread Rust model, using asyncio tasks
- Two concurrent strategies (Ladder MM + Momentum) with independent positions/risk
- Two execution modes: `dry_run` (realistic maker simulation) and `live`
- Replay is separate tooling, not a bot mode
- Textual TUI dashboard mirroring the Nim FTXUI dashboard
- Complete telemetry: binary tape, structured JSON logging, rolling stats
- Discover insights, take notes, then port to Rust with confidence

## 2. Non-Goals

- Performance optimization (Python is the prototype, Rust is production)
- Custom crypto (use `py-clob-client` for all signing/auth)
- Multi-process or shared memory (single-process asyncio is sufficient)
- Replacing the Nim POC (it remains as observation/reference)

---

## 3. Dependencies

```
py-clob-client    — Polymarket CLOB API (signing, orders, auth, heartbeat)
websockets        — PM market WS + PM user channel WS + BN WS feeds
textual           — TUI dashboard
tomli             — TOML config parsing (Python 3.11+ has tomllib in stdlib)
python-dotenv     — .env credential loading
```

### 3.1 WebSocket Endpoints

- **PM market data:** `wss://ws-subscriptions-clob.polymarket.com/ws/market` — subscribe to `market` channel per asset_id for book snapshots, price changes, last_trade_price
- **PM user channel:** `wss://ws-subscriptions-clob.polymarket.com/ws/user` — authenticated via API key headers, receives fills, order updates, trade confirmations
- **Binance streams:** `wss://stream.binance.com:9443/stream?streams=...` — combined stream for bookTicker, trade, depth20@100ms per symbol

### 3.2 Authentication

- **EOA wallet** (`signature_type=0`): private key signs EIP-712 ClobAuth to derive API credentials
- **L2 auth headers** on every authenticated request: `POLY_ADDRESS`, `POLY_API_KEY`, `POLY_PASSPHRASE`, `POLY_SIGNATURE` (HMAC-SHA256), `POLY_TIMESTAMP`
- **py-clob-client handles all of this** — just pass private key + credentials at init

---

## 4. Project Structure

```
polymarket-bot-python-poc/
├── config/
│   └── bot.toml                 # Strategy params, risk limits, credentials path
├── src/
│   ├── __init__.py
│   ├── main.py                  # Entry point — discover markets, launch all tasks
│   ├── config.py                # TOML config loader + dataclasses
│   ├── types.py                 # Shared types: HotEvent, OrderIntent, Side, etc.
│   ├── ingest/
│   │   ├── __init__.py
│   │   ├── polymarket.py        # PM market WS (book, trades, price changes)
│   │   ├── binance.py           # BN bookTicker + trades + depth20 per symbol
│   │   └── account.py           # PM user channel WS (fills, acks, rejects)
│   ├── engine/
│   │   ├── __init__.py
│   │   ├── book.py              # Order book (array-indexed by milli-price)
│   │   ├── market_state.py      # Per-instrument state, BBO, staleness, snapshots
│   │   └── engine.py            # Engine loop — drains 3 queues, fans out to strategies
│   ├── strategy/
│   │   ├── __init__.py
│   │   ├── base.py              # Strategy Protocol — on_event() interface
│   │   ├── ladder_mm.py         # Ladder market maker
│   │   ├── momentum.py          # Momentum/directional
│   │   ├── position.py          # Position tracking (signed qty, VWAP, PnL)
│   │   ├── order_tracker.py     # Order state machine (Pending→Live→Filled/Cancelled)
│   │   └── queue_estimator.py   # L2 queue position model
│   ├── execution/
│   │   ├── __init__.py
│   │   ├── executor.py          # Drains intent queue, dispatches to simulator or live
│   │   ├── simulator.py         # SimulatedExchange — queue-aware maker fill model
│   │   ├── markout.py           # Adverse selection tracking (mid at fill + horizons)
│   │   ├── heartbeat.py         # POST /heartbeat every 5s
│   │   └── risk.py              # Risk gate (per-strategy + global) + kill switch
│   ├── telemetry/
│   │   ├── __init__.py
│   │   ├── recorder.py          # Binary tape writer + structured log
│   │   ├── stats.py             # Rolling counters, latency histogram
│   │   └── telemetry.py         # Telemetry loop — drains both queues, builds snapshots
│   └── dashboard/
│       ├── __init__.py
│       └── app.py               # Textual TUI — books, charts, positions, trades, risk
├── pyproject.toml
└── .env                         # PM_API_KEY, PM_SECRET, PM_PASSPHRASE, PM_PRIVATE_KEY
```

Mapping to Rust target:
- `engine/` = `mantis-market-state` + engine thread
- `strategy/` = `mantis-strategy` (Position, OrderTracker, QueueEstimator, RiskLimits)
- `execution/` = future `mantis-execution`
- `telemetry/` = future `mantis-telemetry`
- `ingest/` = bot binary ingest threads

---

## 5. Task Architecture

Single-process asyncio. Each task maps 1:1 to a Rust thread.

```
┌─────────────────────────────────────────────────────────────────────┐
│                    PYTHON BOT (single process, asyncio)             │
│                                                                     │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐       │
│  │ pm_ingest()  │  │ bn_ingest()  │  │ account_ingest()   │       │
│  │ PM market WS │  │ BN 3×WS/sym │  │ PM user channel WS │       │
│  └──────┬───────┘  └──────┬───────┘  └────────┬───────────┘       │
│         │ pm_q             │ ref_q              │ account_q         │
│         └────────┬─────────┘────────────────────┘                   │
│                  ▼                                                   │
│  ┌─────────────────────────────────────────────────────────┐       │
│  │                    engine_loop()                         │       │
│  │  DRAIN: 1. account_q  2. pm_q  3. ref_q                │       │
│  │  Fan out to all strategies → risk gate → intent_q       │       │
│  │  Emit engine events → telemetry_q                       │       │
│  └──────────┬──────────────────────┬───────────────────────┘       │
│             │ intent_q              │ telemetry_q                    │
│             ▼                       ▼                                │
│  ┌────────────────────┐  ┌──────────────────────────────┐          │
│  │ execution_loop()   │  │ telemetry_loop()              │          │
│  │ global risk gate   │  │ merges telemetry_q +           │          │
│  │ py-clob-client     │  │        exec_telemetry_q       │          │
│  │ sign + POST/DELETE │  │ → tape + log + stats + dash_q │          │
│  │ order state machine│  └──────────────┬────────────────┘          │
│  │  │                 │                  │ dash_q                    │
│  │  │ exec_telemetry_q│                  ▼                          │
│  │  └─────────────────┼──► telemetry   Textual TUI                 │
│  └────────────────────┘                                             │
│  ┌────────────────────┐                                             │
│  │ heartbeat_loop()   │                                             │
│  │ POST every 5s      │                                             │
│  │ fail → PAUSE       │                                             │
│  └────────────────────┘                                             │
└─────────────────────────────────────────────────────────────────────┘
```

**Queues** (all `asyncio.Queue`):

| Queue | Producer | Consumer | maxsize |
|-------|----------|----------|---------|
| pm_q | pm_ingest | engine | 65536 |
| ref_q | bn_ingest | engine | 65536 |
| account_q | account_ingest | engine | 4096 |
| intent_q | engine | execution | 4096 |
| telemetry_q | engine | telemetry | 16384 |
| exec_telemetry_q | execution | telemetry | 4096 |
| dash_q | telemetry | dashboard | 256 |

---

## 6. Types

### 6.1 HotEvent

```python
@dataclass(slots=True)
class HotEvent:
    recv_ts: int              # nanosecond timestamp
    seq: int                  # monotonic sequence
    instrument_id: int        # internal index
    source: EventSource       # PM, BN_BBO, BN_TRADE, BN_DEPTH, ACCOUNT
    kind: EventKind           # BOOK_DELTA, TRADE, TOP_OF_BOOK, ORDER_ACK, FILL, REJECT, etc.
    side: Side
    price: Decimal
    qty: Decimal
    flags: int                # IS_SNAPSHOT, LAST_IN_BATCH
    # Optional fields per kind:
    client_order_id: int = 0
    exchange_order_id: str = ""
    trade_status: str = ""    # MATCHED, CONFIRMED, FAILED
    reject_reason: str = ""
```

### 6.2 OrderIntent

```python
@dataclass(slots=True)
class OrderIntent:
    instrument_id: int
    side: Side
    price: Decimal
    qty: Decimal
    action: OrderAction       # POST, CANCEL, AMEND
    client_order_id: int
    target_order_id: int = 0  # for CANCEL/AMEND
    strategy_id: int = 0
```

### 6.3 Enums

```python
class Side(IntEnum):
    BUY = 0
    SELL = 1

class OrderAction(IntEnum):
    POST = 0
    CANCEL = 1
    AMEND = 2

class EventKind(IntEnum):
    BOOK_DELTA = 0
    TRADE = 1
    TOP_OF_BOOK = 2
    ORDER_ACK = 3
    FILL = 4
    ORDER_REJECT = 5
    HEARTBEAT = 6
    TIMER = 7

class KillState(IntEnum):
    ACTIVE = 0
    REDUCE_ONLY = 1
    PAUSED = 2
    KILLED = 3
```

---

## 7. Strategy

### 7.1 Strategy Protocol

```python
class Strategy(Protocol):
    strategy_id: int
    name: str

    def on_event(self, event: HotEvent) -> list[OrderIntent]:
        """Process one event. Returns 0-N order intents."""
        ...
```

Each strategy owns:
- `MarketState` — book reconstruction per instrument
- `Position` — signed qty, VWAP entry, realized/unrealized PnL
- `OrderTracker` — order state machine (Pending/Sending/Live/Filled/Cancelled/Rejected)
- `QueueEstimator` — L2 probabilistic queue model (PowerProbQueueFunc)
- `RiskLimits` — per-strategy limits from config

### 7.2 Position

```python
@dataclass
class Position:
    instrument_id: int
    qty: int                  # signed: positive=long, negative=short
    avg_entry: Decimal
    realized_pnl: Decimal
    fill_count: int

    def on_fill(self, side: Side, qty: Decimal, price: Decimal): ...
    def unrealized_pnl(self, current_mid: Decimal) -> Decimal: ...
    def total_pnl(self, current_mid: Decimal) -> Decimal: ...
```

Flat invariant: when qty reaches 0, avg_entry resets to zero.

### 7.3 OrderTracker

```python
class OrderState(IntEnum):
    PENDING = 0
    SENDING = 1
    LIVE = 2
    CANCEL_PENDING = 3
    FILLED = 4
    CANCELLED = 5
    REJECTED = 6

@dataclass
class TrackedOrder:
    client_order_id: int
    exchange_order_id: str
    strategy_id: int
    instrument_id: int
    side: Side
    price: Decimal
    original_qty: Decimal
    filled_qty: Decimal
    state: OrderState
    created_at_ns: int
    acked_at_ns: int          # 0 = not yet acked
```

Fixed-size tracker (max 64 orders). Slot reclamation on terminal states.

### 7.4 QueueEstimator

L2 queue position estimator using PowerProbQueueFunc (n=2-3).
- `register_order()` — track new order at level
- `on_trade()` / `on_level_change()` — update queue ahead estimates
- `queue_ahead(order_id)` → estimated contracts ahead
- `fill_probability(order_id, time_remaining)` → P[fill before expiry]
- Poisson model: `P[Poisson(mu * T) >= q]` where mu = take rate

### 7.5 Ladder MM Strategy

- Post symmetric ladder around fair value (N levels each side from config)
- Fair value = weighted mid from PM book + Binance reference skew
- Inventory skew: shift ladder away from current position
- Diff-based repricing: only cancel/repost changed levels, preserve queue on unchanged
- Triggers on BBO change (edge-triggered via `take_tob()`)

### 7.6 Momentum Strategy

- Watch Binance reference price for strong directional moves
- Signal: reference price move exceeds threshold within lookback window
- Post single directional order on strong signal
- Max one pending order at a time
- On fill: optionally post insurance hedge on opposite side

---

## 8. Execution

Two modes: `dry_run` and `live`. No "paper mode" — the dry_run IS the paper mode, but realistic.

### 8.1 Execution Loop

```
drain intent_q → accumulate batches:
  POST intents → sign via py-clob-client → batch (up to 15)
  CANCEL intents → batch cancel

global risk gate (Layer 3) before signing:
  kill switch state
  wallet budget reservation
  feed health
  rate limits

execute: DELETE first, then POST (stale quotes > gap)

update order state machine:
  PENDING → SENDING → LIVE / REJECTED
  LIVE → MATCHED → CONFIRMED (or FAILED → rollback)
  LIVE → CANCEL_PENDING → CANCELLED

emit to exec_telemetry_q:
  order placed, fill, cancel, risk reject, API latency, heartbeat
```

### 8.2 Dry Run — Realistic Maker Execution Simulator

`mode = "dry_run"` in config. This is a **maker-only execution simulator**, not a naive paper trader.

**Design principle:** The dry_run must be skeptical by default. A strategy should look *worse* in dry_run than live (pessimistic fills), never better. If a strategy is profitable under realistic dry_run, it has a good chance of working live.

**Core module:** `src/execution/simulator.py` — `SimulatedExchange`

#### 8.2.1 Order Lifecycle (dry_run)

```
Strategy emits OrderIntent
  │
  ├─ Latency injection: order is PENDING for submit_latency_ms
  │   (sampled from LatencyProfile, default ~200ms)
  │   Book may move during this window. Strategy cannot see order on book.
  │
  ▼
SimulatedExchange receives order at T + submit_latency
  │
  ├─ Post-only check: is order marketable at arrival-time book state?
  │   YES, action=POST → REJECTED (emit reject event via account_q)
  │       (or: simulate as taker fill with taker fee, configurable)
  │   NO → order joins queue at BACK of price level
  │
  ├─ Ack emitted after ack_latency_ms (PENDING → LIVE)
  │
  ├─ Queue tracking begins:
  │   queue_ahead = level_size_at_arrival - my_size
  │   Updated on every observed trade and cancel at this price level
  │
  ├─ Fill trigger (on each observed trade at our price level):
  │   aggressive_qty consumed from queue front (FIFO)
  │   if queue_ahead <= 0:
  │     my_fill = min(remaining_size, aggressive_qty_after_queue)
  │     → partial or full fill
  │     → emit FillEvent via account_q
  │     → record markout snapshot (mid at fill time)
  │     → schedule markout measurements at +100ms, +500ms, +1s, +5s
  │
  ├─ Cancel request:
  │   cancel_latency_ms before order removed from simulated book
  │   If fill arrives during cancel window → fill wins (race condition)
  │
  └─ Cancel/Repost (Amend):
      New order goes to BACK of queue at new price level
      Queue advantage at old level is LOST — this is critical
```

#### 8.2.2 Queue Model

```python
@dataclass
class SimOrder:
    order_id: int
    instrument_id: int
    side: Side
    price: Decimal
    qty: Decimal
    queue_ahead: Decimal          # estimated contracts ahead of us (FIFO)
    fill_qty: Decimal = Decimal(0)
    arrival_time_ns: int = 0
    state: OrderState = OrderState.PENDING

@dataclass
class SimulatedLevel:
    price: Decimal
    side: Side
    our_orders: list[SimOrder]    # our resting orders at this level (FIFO)
```

Queue updates on observed market data:
- **Trade at our price:** `queue_ahead -= trade_qty`. When `queue_ahead <= 0`, our order fills (partially or fully).
- **Level size decrease (cancel):** Use PowerProbQueueFunc — cancels are biased toward back of queue. `prob_ahead = (queue_ahead / level_size_at_insertion) ^ n` where n=2-3. `queue_ahead -= removed * prob_ahead`.
- **Level size increase (new order):** Goes behind us, no effect on queue_ahead.

#### 8.2.3 Latency Model

```python
@dataclass
class LatencyProfile:
    submit_mean_ms: float = 200.0
    submit_std_ms: float = 50.0
    cancel_mean_ms: float = 150.0
    cancel_std_ms: float = 40.0
    ack_mean_ms: float = 50.0
    ack_std_ms: float = 20.0

    def sample_submit(self) -> float:
        return max(10.0, random.gauss(self.submit_mean_ms, self.submit_std_ms))

# Config profiles:
# [latency]
# profile = "realistic"  # optimistic | realistic | pessimistic
#
# optimistic:  submit=100ms, cancel=80ms
# realistic:   submit=250ms, cancel=150ms
# pessimistic: submit=400ms, cancel=250ms
```

#### 8.2.4 Partial Fills

Fills are almost always partial for makers. When a taker trades 50 shares at our level and our order is 100 shares with queue_ahead=0:
- Fill 50 shares (partial)
- 50 shares remain resting with queue_ahead=0 (front of queue now)
- Multiple partial fills accumulate until full fill

#### 8.2.5 Post-Only Safety

At simulated arrival time, check if order would cross the book:
- BUY order at price >= best_ask → **REJECT** (would be taker)
- SELL order at price <= best_bid → **REJECT** (would be taker)

Configurable: `post_only_reject = true` (reject) or `false` (simulate taker fill with taker fee applied).

#### 8.2.6 Adverse Selection / Markout Tracking

```python
@dataclass
class Markout:
    fill_time_ns: int
    fill_price: Decimal
    fill_side: Side
    fill_qty: Decimal
    instrument_id: int
    mid_at_fill: Decimal
    mid_at_100ms: Decimal | None = None
    mid_at_500ms: Decimal | None = None
    mid_at_1s: Decimal | None = None
    mid_at_5s: Decimal | None = None
```

Markout measures: "after I got filled, did the price move for or against me?"
- Positive markout = good fill (price moved in our favor after fill)
- Negative markout = toxic fill (price moved against us — we provided cheap liquidity to informed flow)
- This is the #1 metric for whether a maker strategy has real edge

#### 8.2.7 PnL Breakdown

```python
@dataclass
class PnLBreakdown:
    execution_pnl: Decimal        # realized from closed fills
    mark_to_market_pnl: Decimal   # unrealized on open inventory
    taker_fees_paid: Decimal      # from accidental taker fills (should be ~0)
    estimated_rebate: Decimal     # C × feeRate × p(1-p) × rebate_pct per maker fill
    total_pnl: Decimal            # execution + mtm (WITHOUT rebates)
    total_with_rebates: Decimal   # total + rebate - fees
```

**Primary display: `total_pnl` (without rebates).** Rebates are separate income. A strategy that is only profitable because of rebates is rebate farming, not market making.

#### 8.2.8 Dry Run Metrics (displayed on dashboard)

```
EXECUTION SIMULATOR METRICS:
  Fill rate:           12.3% of posted quotes filled
  Time to first fill:  p50=4.2s  p99=28s
  Partial fill rate:   34% of fills are partial
  Queue position:      avg 340 ahead at insertion, avg 80 at fill
  Post-only rejects:   2.1% of quotes became marketable on arrival
  Cancel/repost count: 47 reprices (all lost queue priority)
  Adverse selection:   markout@1s = -0.3bps (CAUTION)
  PnL (no rebates):    -$12.40
  PnL (with rebates):  +$3.20
  Taker fees:          $0.00
  Est. rebates:        $15.60
```

#### 8.2.9 Simulation Profiles (TOML config)

```toml
[dry_run]
enabled = true
latency_profile = "realistic"    # optimistic | realistic | pessimistic
queue_power_n = 3.0              # PowerProbQueueFunc parameter
post_only_reject = true          # reject marketable quotes (vs taker fill)
settlement_fail_rate = 0.005     # 0.5% of MATCHED trades FAIL
markout_horizons_ms = [100, 500, 1000, 5000]
```

### 8.3 Live Mode

`mode = "live"` in config:
- Full execution via `py-clob-client`
- Sign + batch POST/DELETE to Polymarket API
- Real fills via user channel WS
- Heartbeat every 5s
- All telemetry + markout tracking still active (compare live vs dry_run)

### 8.4 Heartbeat

- POST /heartbeat every 5s via `py-clob-client`
- Track heartbeat_id chain (response contains next ID)
- On failure > 8s → kill switch PAUSED
- Emits heartbeat status to exec_telemetry_q
- Skipped in dry_run mode

### 8.5 Kill Switch

States: `ACTIVE → REDUCE_ONLY → PAUSED → KILLED`

Automatic triggers:
- Heartbeat fail > 8s → PAUSED
- Feed stale > 5s → PAUSED
- Wallet budget breach → PAUSED
- Runaway intent rate → REDUCE_ONLY
- Repeated API errors → REDUCE_ONLY

PAUSED auto-cancels all orders (live) or clears simulated book (dry_run). KILLED requires manual restart.

---

## 9. Risk Management

### 9.1 Three Layers

```
Layer 1: Strategy self-check (advisory, inside on_event)
  - Position limits, max loss, feed health

Layer 2: Risk gate (engine_loop, between strategy and intent_q)
  - Per-strategy: rate, position, capital, feed health
  - Reject + log on fail

Layer 3: Global gate (execution_loop, before signing)
  - Kill switch state
  - Wallet budget reservation (atomic)
  - Global rate limits
  - Feed health
```

### 9.2 Per-Strategy Limits (from config)

```python
@dataclass
class RiskLimits:
    max_position_per_instrument: int
    capital_budget: Decimal
    max_worst_case_loss: Decimal
    max_orders_live: int
    max_intents_per_sec: int
    max_replaces_per_min: int
    quote_age_ms: int
```

### 9.3 Global Limits (from config)

```python
@dataclass
class GlobalRiskLimits:
    wallet_min_free_usdc: Decimal
    max_live_orders_total: int
    max_submits_per_min: int
    max_cancels_per_min: int
    feed_stale_ms: int
    feed_dead_ms: int
```

### 9.4 Feed Watchdog

- 500ms stale: warning log
- 1s: block new quoting
- 2s: cancel existing quotes
- 5s: kill switch → PAUSED
- Recovery: 2s healthy + fresh snapshot → ACTIVE

---

## 10. Telemetry

### 10.1 Inputs

Merges two queues:
- `telemetry_q` — engine events (market data processed, strategy intent counts)
- `exec_telemetry_q` — execution events (orders, fills, cancels, rejects, API timing, heartbeat)

### 10.2 Outputs

**Binary tape** (file writes):
- `input_tape`: every event for replay
- `state_tape`: BBO changes only

**Structured log** (`mantis.log`, JSON lines):
```json
{"ts":"16:23:45.123","level":"INFO","type":"FILL","strategy":"ladder_mm","side":"BUY","qty":100,"price":"0.52"}
{"ts":"16:23:45.200","level":"WARN","type":"RISK_REJECT","strategy":"momentum","reason":"position_limit"}
{"ts":"16:23:46.001","level":"INFO","type":"API_CALL","endpoint":"batch_post","count":3,"latency_ms":142}
```

Event types: FILL, ORDER_PLACED, ORDER_CANCELLED, RISK_REJECT, GLOBAL_REJECT, API_CALL, FEED_ALERT, HEARTBEAT, KILL_SWITCH, SLOW_EVENT, RECONCILE

**Rolling stats** (in-memory):
- Latency histogram (p50/p95/p99/p999)
- Event rate counters per source
- Per-strategy: intent count, fill count, reject count
- Queue depth samples

**Dashboard snapshot** (10Hz → dash_q):
- All stats needed for Textual TUI rendering

**Periodic reconciliation** (every 30s):
- Compare local order/position state vs Polymarket REST API
- Log discrepancies

---

## 11. Dashboard (Textual TUI)

3-column layout inspired by Nim FTXUI dashboard:

**Left column:**
- PM order book (milli-price levels, color-coded bid/ask, 8 levels per side)
- Binance reference prices (BBO per symbol)
- Market trade tape (last 8 trades with BUY/SELL coloring)

**Center column:**
- Probability chart (weighted mid, sparkline, 60s window)
- Latency histogram (p50/p95/p99/p999 + bar chart)
- Queue depth sparklines (pm_q, ref_q, account_q, intent_q)
- Event rate counters (PM, BN, Account events/sec)

**Right column:**
- Per-strategy panels (PnL, position, live orders, fills, rejects)
- Risk status (kill switch state, heartbeat health, feed staleness, wallet balance, intent rate)
- Execution tape (recent API calls with latency, fills, rejects)

**Controls:**
- Keys 1-9: switch market tabs
- `p`: toggle paper/live mode indicator
- `q`: graceful quit (cancel all orders, flush tapes)

---

## 12. Market Discovery & Lifecycle

```
main.py:
  1. Load config (bot.toml + .env)
  2. Initialize py-clob-client (EOA wallet)
  3. Discover markets (BTC/SOL/ETH up-or-down, filter by timeframe)
  4. Build instrument registry
  5. Wait for next window open
  6. Launch all asyncio tasks + Textual app
  7. Run until window closes (or ctrl-c)
  8. Graceful shutdown:
     - Kill switch → PAUSED
     - Cancel all open orders
     - Stop heartbeat
     - Flush tapes + logs
     - Print summary report
  9. Loop → discover next window → goto 5
```

### 12.1 Instrument Registry

```python
@dataclass
class Instrument:
    instrument_id: int        # internal index (0-based)
    token_id: str             # PM ERC1155 token ID
    condition_id: str         # market condition ID
    outcome: str              # "YES" / "NO"
    tick_size: Decimal        # 0.01, 0.001, etc.
    neg_risk: bool
    min_size: Decimal
    asset: str                # "BTC", "SOL", "ETH"
    timeframe: str            # "5m", "15m"
    ref_symbol: str           # "BTCUSDT", "SOLUSDT", "ETHUSDT"
```

---

## 13. TOML Config

```toml
[general]
mode = "dry_run"              # dry_run | live
timeframe = "5m"
assets = ["BTC", "SOL", "ETH"]
log_level = "INFO"
tape_dir = "data/tapes"

[credentials]
env_file = ".env"

[strategy.ladder_mm]
enabled = true
capital_budget = 500.0
levels = 5
spread_bps = 50
skew_factor = 0.3
max_position = 1000
max_orders_live = 20
max_replaces_per_min = 100
quote_age_ms = 2000

[strategy.momentum]
enabled = true
capital_budget = 300.0
signal_threshold = 0.7
max_position = 500
max_orders_live = 2
max_replaces_per_min = 5

[risk.global]
wallet_min_free_usdc = 100.0
max_live_orders_total = 50
max_submits_per_min = 200
max_cancels_per_min = 300
feed_stale_ms = 2000
feed_dead_ms = 5000

[heartbeat]
interval_s = 5
fail_pause_after_s = 8

[dry_run]
latency_profile = "realistic"    # optimistic | realistic | pessimistic
queue_power_n = 3.0              # PowerProbQueueFunc cancel bias
post_only_reject = true          # reject marketable quotes on arrival
settlement_fail_rate = 0.005     # 0.5% MATCHED→FAILED
markout_horizons_ms = [100, 500, 1000, 5000]

[dashboard]
enabled = true
refresh_hz = 10
```

---

## 14. What This Proves for Rust

| Python POC | Rust Target |
|---|---|
| asyncio task | OS thread (pinned core) |
| asyncio.Queue | mantis-queue SPSC ring |
| HotEvent dataclass | mantis-events HotEvent (64B repr(C)) |
| Strategy Protocol | mantis-strategy Strategy trait |
| Position/OrderTracker | mantis-strategy components |
| execution_loop | mantis-execution crate |
| telemetry_loop | mantis-telemetry crate |
| py-clob-client | native libsecp256k1 + HTTP client |
| TOML config | same TOML, different parser |

The Python POC validates:
- Multi-strategy fan-out and routing works
- 3-layer risk model catches the right things
- Queue position estimation improves fill rates
- Kill switch state machine handles all failure modes
- Telemetry pipeline captures everything needed for replay
- Dashboard layout shows the right information
- **Dry_run simulator is realistic enough to predict live PnL** — queue model, latency, partial fills, adverse selection
- **Markout tracking detects toxic flow** before going live with real money
- **PnL without rebates** is the true measure of strategy edge

Insights and edge cases discovered here feed directly into the Rust implementation.
