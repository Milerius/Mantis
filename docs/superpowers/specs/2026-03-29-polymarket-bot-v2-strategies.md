# Polymarket Trading Bot — V2 Strategy Redesign Spec

**Date**: 2026-03-29
**Status**: Draft
**Branch**: `feature/polymarket-bot`
**Supersedes**: Section "pm-signal" of the original spec. All other sections (workspace layout, pm-types, pm-oracle, pm-bookkeeper, pm-executor, pm-market, pm-risk, phases) remain valid.

---

## Why V2

The Phase 1 backtest produced results ($500 → $11,125 in one week) that are **misleading**. The backtest simulated market prices as `fair_value - offset`, which is circular — it assumes you can buy at a price that gives you the edge you're testing for.

Real-world evidence from 176-trade bot analysis (dev.to), 0xInsider's 10,582 trader study, and inspection of the target trader (0xe1D6...) reveals:

1. **By minute 8-12 of a 15m window**, Polymarket contracts are priced at $0.85-0.99 when there's a clear trend. The edge is gone.
2. **The binary math trap**: at entry price $0.85, you need 85% win rate to break even. No bot achieves this consistently on directional trades.
3. **Profitable traders enter EARLY** (minute 0-3) at $0.44-0.55. They predict direction before the market reprices, not after.
4. **Complete-set arbitrage** (buy YES + NO < $1.00) is the only strategy with mathematically guaranteed profit, but margins are $0.01-0.03/trade.
5. **Our backtest needs real Polymarket price data**, not simulated prices.

---

## V2 Strategy Architecture

### Core Trait Change

Replace `FairValueEstimator` (single probability estimate) with `Strategy` (full entry decision including price awareness):

```rust
/// Market state snapshot provided to all strategies on every tick.
pub struct MarketState {
    pub asset: Asset,
    pub timeframe: Timeframe,
    pub window_id: WindowId,
    pub window_open_price: Price,         // spot at window open
    pub current_spot: Price,              // latest exchange tick
    pub spot_magnitude: f64,              // abs(current - open) / open
    pub spot_direction: Side,             // Up or Down
    pub time_elapsed_secs: u64,           // seconds since window opened
    pub time_remaining_secs: u64,         // seconds until close
    pub contract_ask_up: Option<ContractPrice>,   // PM best ask for Up
    pub contract_ask_down: Option<ContractPrice>,  // PM best ask for Down
    pub contract_bid_up: Option<ContractPrice>,    // PM best bid for Up
    pub contract_bid_down: Option<ContractPrice>,  // PM best bid for Down
}

/// A decision to enter a position.
pub struct EntryDecision {
    pub side: Side,
    pub limit_price: ContractPrice,       // max price we'll pay
    pub confidence: f64,                  // 0-1, influences Kelly sizing
    pub strategy_id: StrategyId,          // which strategy produced this
}

/// Strategy identifier for logging and P&L attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrategyId {
    CompleteSetArb,
    EarlyDirectional,
    MomentumConfirmation,
    HedgeLock,
}

/// A strategy evaluates market state and optionally produces an entry decision.
pub trait Strategy {
    fn id(&self) -> StrategyId;
    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision>;
}
```

### Strategy A: Complete-Set Arbitrage

**What**: Buy both YES and NO when combined cost < $1.00. Guaranteed profit regardless of outcome.

**When**: Any time during a window, whenever `ask_up + ask_down < threshold` (e.g., $0.98).

**Entry price**: Both sides at their respective ask prices. Total cost must be < $1.00.

**Edge**: Mathematical, not statistical. A $0.02 gap is $0.02 profit per share.

**Risk**: Half-fills — one side fills, other doesn't. Now you have a naked directional position.

**Mitigation**: Fill-or-kill on both legs. If first leg fills but second fails, immediately sell the first leg.

**Expected**: $0.01-0.03/trade, 95%+ win rate, hundreds of trades/day. The $150K arb bot did this at scale.

**Backtest requirement**: Needs real Polymarket orderbook data (bid/ask for both sides). Download historical trades from Polymarket Data API and reconstruct bid/ask at each timestamp.

```rust
pub struct CompleteSetArb {
    /// Maximum combined cost to trigger arb (e.g., 0.98 = $0.02 min profit)
    pub max_combined_cost: f64,
    /// Minimum profit per share to bother (e.g., 0.015)
    pub min_profit_per_share: f64,
}

impl Strategy for CompleteSetArb {
    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        let ask_up = state.contract_ask_up?.as_f64();
        let ask_down = state.contract_ask_down?.as_f64();
        let combined = ask_up + ask_down;
        if combined >= self.max_combined_cost {
            return None;
        }
        let profit_per_share = 1.0 - combined;
        if profit_per_share < self.min_profit_per_share {
            return None;
        }
        // Buy the cheaper side as primary (other side handled by executor)
        // This is a special case — executor needs to know it's an arb
        // and must buy BOTH sides
        Some(EntryDecision {
            side: if ask_up <= ask_down { Side::Up } else { Side::Down },
            limit_price: ContractPrice, // the cheaper side's ask
            confidence: 1.0,            // arb = max confidence
            strategy_id: StrategyId::CompleteSetArb,
        })
    }
}
```

### Strategy B: Early Directional (the 0xe1D6 play)

**What**: Enter in the first 1-3 minutes of a window when spot price has moved but the Polymarket contract hasn't fully repriced.

**When**: `time_elapsed < 180s` AND `spot_magnitude > min_threshold` AND `contract_ask < max_entry_price`.

**Entry price**: The current Polymarket ask. The key is that early in the window, the ask is still $0.50-0.60 even when spot has moved.

**Edge**: Speed. We detect the spot movement on Binance/OKX faster than Polymarket reprices. The window of opportunity is 10-60 seconds.

**Risk**: BTC reverses. At entry $0.55, a loss is $0.55/share. Win is $0.45/share. Need >55% win rate.

**Why this works**: The target trader (0xe1D6) enters at avg $0.44 on BTC 8AM daily windows. His best trade: +$4,720 on a $3,691 investment (128% return). He enters EARLY.

**Backtest requirement**: Map (spot_magnitude, time_elapsed) → actual Polymarket contract price from historical data. Then test: "if we entered at the real contract price at minute 2, what's our win rate?"

```rust
pub struct EarlyDirectional {
    /// Maximum seconds after window open to consider entry (e.g., 180 = 3 min)
    pub max_entry_time_secs: u64,
    /// Minimum spot movement to trigger (e.g., 0.001 = 0.1%)
    pub min_spot_magnitude: f64,
    /// Maximum contract price to pay (e.g., 0.60)
    pub max_entry_price: f64,
}
```

**Critical parameters to test in backtest**:
- `max_entry_time_secs`: 60, 120, 180, 300 — earlier = better price but less signal certainty
- `min_spot_magnitude`: 0.0005, 0.001, 0.002, 0.003 — lower = more trades but more noise
- `max_entry_price`: 0.50, 0.55, 0.60, 0.65 — lower = better risk/reward but fewer entries

### Strategy C: Momentum Confirmation

**What**: Enter mid-window (minute 5-10) when a sustained move is confirmed.

**When**: `time_elapsed between 300-600s` AND `spot_magnitude > 0.003` AND `contract_ask < 0.72`.

**Entry price**: Higher than Strategy B ($0.65-0.72). Worse risk/reward but higher conviction.

**Edge**: Statistical. A 0.3% sustained move over 5 minutes is more likely to hold than a 0.1% move over 1 minute.

**Risk**: At $0.72 entry, need >72% win rate. The 176-trade guy showed real-world momentum WR is ~80% which is borderline.

**Why include it**: It catches the moves that Strategy B missed (either because B's entry didn't trigger, or B's max_entry_price was exceeded). Also catches continuation moves where adding to a winning position makes sense.

**Backtest**: Same data as Strategy B. Test: "at minute 5-10, when spot has moved 0.3%+, if we enter at the real contract price, what's the win rate?"

```rust
pub struct MomentumConfirmation {
    /// Earliest entry time (e.g., 300 = minute 5)
    pub min_entry_time_secs: u64,
    /// Latest entry time (e.g., 600 = minute 10)
    pub max_entry_time_secs: u64,
    /// Minimum spot movement (e.g., 0.003 = 0.3%)
    pub min_spot_magnitude: f64,
    /// Maximum contract price to pay (e.g., 0.72)
    pub max_entry_price: f64,
}
```

### Strategy D: Hedge Lock

**What**: If holding a losing position, buy the opposite side to lock in a guaranteed profit or reduce loss.

**When**: Existing position is losing AND `my_entry + opposite_ask < 1.00`.

**Not a standalone strategy** — it triggers when other strategies' positions are underwater.

**Example**: Bought Up at $0.55 (Strategy B), BTC reversed. Down is now at $0.40. Total: $0.55 + $0.40 = $0.95 < $1.00. Buy Down → guaranteed $0.05 profit regardless of outcome.

```rust
pub struct HedgeLock {
    /// Maximum combined cost to trigger hedge (e.g., 0.98)
    pub max_combined_cost: f64,
}
```

---

## Realistic Backtest Design

### Data Requirements

The backtest needs TWO data sources aligned by timestamp:

1. **Binance/OKX spot data** (already have): 1-second candles showing actual BTC/ETH price at each second.

2. **Polymarket trade data** (NEW): Historical trades on BTC/ETH Up/Down 15m markets showing actual contract prices at each second within the window.

### Polymarket Historical Data Pipeline

The Polymarket Data API provides trade data:
```
GET https://data-api.polymarket.com/trades?market={slug}&limit=100
```

Each trade has: `side`, `price`, `size`, `timestamp`, `outcome` (Up/Down).

**Download strategy**: For each historical BTC/ETH 15m window we have spot data for:
1. Construct the market slug: `btc-updown-15m-{epoch_of_window_open}`
2. Fetch all trades for that market
3. Reconstruct contract price time series: at each second, the last trade price is the "market price"
4. Cache alongside spot data

**Limitation**: Not all windows will have trade data (thin markets). We'll mark these as "no data" and skip in backtest.

### Contract Price Model (for windows without trade data)

For windows where Polymarket trade data is sparse, we build an empirical model:

```
contract_price_estimate = f(spot_magnitude, time_elapsed, asset, timeframe)
```

Calibrated from windows WHERE we have both spot and trade data. This gives us a larger backtest sample, but we always validate against real trade data first.

**Model structure**: Lookup table bucketed by (spot_magnitude, time_elapsed). Each cell stores the empirical median contract price from real Polymarket data.

### Backtest Flow

```
For each historical window (15m, 1h, 4h):
  1. Load spot data (Binance 1s candles)
  2. Load Polymarket trade data (if available) OR use model estimate
  3. At each second, construct MarketState:
     - spot from Binance
     - contract price from Polymarket trades (or model)
  4. Run all 4 strategies against MarketState
  5. If any strategy fires, record entry at the ACTUAL contract price
     (not a simulated one)
  6. At window close, resolve: did it go Up or Down?
  7. Compute P&L per strategy
```

**Key output**: per-strategy breakdown
```
Strategy A (Arb):        X trades, Y% WR, $Z P&L, avg entry $W
Strategy B (Early Dir):  X trades, Y% WR, $Z P&L, avg entry $W
Strategy C (Momentum):   X trades, Y% WR, $Z P&L, avg entry $W
Strategy D (Hedge):      X trades, Y% WR, $Z P&L, avg entry $W
Combined:                X trades, Y% WR, $Z P&L
```

### Parameter Sweep

The backtest should sweep key parameters to find the optimal configuration:

**Strategy B sweep**:
```
max_entry_time_secs: [60, 120, 180, 300]
min_spot_magnitude: [0.0005, 0.001, 0.002, 0.003]
max_entry_price: [0.50, 0.55, 0.60, 0.65, 0.70]
```
= 4 × 4 × 5 = 80 parameter combinations

**Strategy C sweep**:
```
min_entry_time_secs: [180, 300, 420]
max_entry_time_secs: [420, 600, 720]
min_spot_magnitude: [0.002, 0.003, 0.005]
max_entry_price: [0.65, 0.70, 0.75]
```
= 3 × 3 × 3 × 3 = 81 parameter combinations

Each combination runs the full backtest. This is where Rust speed matters — 160+ backtests need to complete in minutes, not hours.

---

## Performance Optimizations

### What Matters (end-to-end latency)

The Rust advantage is NOT in signal math (already 1.7-17ns). It's in the full path from exchange tick to Polymarket order:

```
Exchange WebSocket → parse tick → evaluate strategies → sign order → POST to CLOB
```

| Component | Python | Rust | Improvement |
|---|---|---|---|
| WebSocket message parse | 1-5ms | 10-50µs | 20-100x |
| Strategy evaluation | 0.1-1ms | 1.7-17ns | 60,000x (irrelevant) |
| EIP-712 order signing | 5-10ms | 0.5-1ms | 10x |
| HTTP POST (shared conn) | 10-20ms | 5-10ms | 2x |
| **Total tick-to-order** | **16-36ms** | **5.5-11ms** | **2-3x** |

At minute 2 of a 15m window, contract prices move ~$0.01/second. A 20ms advantage means entering $0.02 cheaper — that's $0.02/share × 100 shares = $2 per trade × 100 trades/day = $200/day from speed alone.

### Specific Optimizations to Implement

1. **Zero-copy WebSocket parsing**: Use `simd-json` or custom extractor for Binance/OKX ticks. Only extract `price` and `timestamp` fields, skip everything else.

2. **mantis-queue SPSC ring**: Replace tokio broadcast channel for tick distribution. Oracle task pushes to ring, signal task reads. ~2ns per tick vs ~500ns for mpsc.

3. **Pre-computed EIP-712 templates**: Build the typed data structure once at startup, fill in price/size at trade time. Saves ~0.5ms per order.

4. **HTTP connection pooling**: Keep-alive connections to Polymarket CLOB. First order ~50ms (TLS), subsequent ~5ms.

5. **Batch tick processing**: Process all ticks from the same millisecond as a batch, evaluate all windows, then place orders. Prevents interleaving.

6. **Hot-path `#[repr(C)]` structs**: `MarketState` and `EntryDecision` are cache-line aligned, no indirection, no heap allocation on the hot path.

---

## Crate Changes

### pm-signal (reworked)

```
pm-signal/src/
  lib.rs              # Re-exports
  strategy.rs         # Strategy trait, MarketState, EntryDecision, StrategyId
  arb.rs              # CompleteSetArb implementation
  early.rs            # EarlyDirectional implementation
  momentum.rs         # MomentumConfirmation implementation
  hedge.rs            # HedgeLock implementation
  engine.rs           # StrategyEngine: runs all strategies, returns best decision

  # KEEP from v1 (used by calibration):
  estimator.rs        # FairValueEstimator trait (for contract price model)
  lookup.rs           # LookupTable (reused for contract price model)
  logistic.rs         # LogisticModel (reused for contract price model)
```

### pm-oracle (additions)

```
pm-oracle/src/
  # KEEP from v1:
  downloader.rs       # Binance REST
  storage.rs          # Compressed cache
  replay.rs           # HistoricalReplay
  price_buffer.rs     # PriceBuffer

  # NEW:
  polymarket.rs       # Polymarket historical trade downloader
  contract_price.rs   # ContractPriceModel: (magnitude, time_elapsed) → estimated contract price
```

### pm-executor (reworked backtest)

```
pm-executor/src/
  backtest.rs         # Reworked: uses real contract prices, runs all 4 strategies
  sweep.rs            # Parameter sweep runner
```

### pm-types (additions)

```
pm-types/src/
  # ADD to market.rs:
  MarketState, EntryDecision, StrategyId

  # ADD to trade.rs:
  strategy_id field on TradeRecord (for per-strategy P&L attribution)
```

---

## Comparison: Target Trader vs Our Strategies

The target trader (0xe1D6..., $89K P&L in 28 days, 3,880 trades):

| Metric | Target Trader | Our Strategy B (Early Dir) | Our Strategy A (Arb) |
|---|---|---|---|
| Entry timing | First 1-5 min of window | First 1-3 min | Any time |
| Avg entry price | $0.44-0.59 | $0.50-0.60 (target) | $0.49 + $0.49 = $0.98 |
| Win rate needed | >50% at $0.50 | >55% at $0.55 | N/A (guaranteed) |
| Trades/day | ~138 | ~50-100 (estimated) | ~50-200 (depends on spreads) |
| Per-trade P&L | ~$23 avg | TBD from backtest | $0.01-0.03/share |
| Assets | BTC, ETH, SOL, XRP | BTC, ETH | BTC, ETH |
| Timeframes | 15m, 1h, 4h | 15m, 1h, 4h | 15m only (fastest recycling) |
| Multi-timeframe | Yes, stacks across | Yes | No |
| Hedging | Yes (buys both sides) | Strategy D | Inherent (both sides) |
