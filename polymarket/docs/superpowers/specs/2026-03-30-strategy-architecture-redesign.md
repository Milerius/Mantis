# Strategy Architecture Redesign — Independent Instances

## Goal

Redesign the strategy execution pipeline so each strategy config becomes a fully independent instance with its own balance, positions, risk management, and P&L. Strategies never block each other. The system supports running hundreds of instances simultaneously.

## Architecture

### Core Trait

```rust
pub trait StrategyInstance: Send + Sync {
    /// Unique label (e.g. "ED-tight", "MC-loose").
    fn label(&self) -> &str;

    /// Evaluate market state and optionally open a position.
    /// Handles risk checks, sizing, and fill internally.
    fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent>;

    /// Resolve positions when a window closes.
    fn on_window_close(
        &mut self,
        window_id: WindowId,
        outcome: Side,
        timestamp_ms: u64,
    ) -> Vec<TradeRecord>;

    /// Current balance.
    fn balance(&self) -> f64;

    /// Session stats for reporting.
    fn stats(&self) -> &InstanceStats;
}
```

### Instance Internals

Each `ConcreteStrategyInstance` owns:

| Component | Purpose |
|-----------|---------|
| `label: StrategyLabel` | Identity for logs and reports |
| `strategy: AnyStrategy` | Pure evaluator (EarlyDir, Momentum, etc.) |
| `balance: f64` | Own capital, starts at `config.balance` |
| `open_positions: Vec<ActivePosition>` | Only this instance's positions |
| `trades: Vec<TradeRecord>` | Trade history for reporting |
| `max_position_usdc: f64` | Per-trade size cap |
| `max_exposure_usdc: f64` | Total open positions cap |
| `kelly_fraction: f64` | Kelly sizing parameter |
| `max_daily_loss: f64` | Daily loss kill switch |
| `daily_pnl: f64` | Running daily P&L |
| `kill_switch: bool` | Auto-triggered on daily loss breach |
| `window_slots: [bool; 32]` | Per-(asset, tf) dedup — one position per window per instance |
| `stats: InstanceStats` | W/L/PnL/biggest win/loss |

~200 bytes hot state per instance. 4 instances = 800 bytes, fits in L1 cache.

**No mutex, no Arc, no shared state between instances.** The main loop owns all instances by value.

### `on_tick` Internal Flow

```
fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent> {
    // 1. Slot check — already have a position for this (asset, tf) window?
    let slot = asset_idx * TF_COUNT + tf_idx;
    if self.window_slots[slot] { return None; }

    // 2. Kill switch
    if self.kill_switch { return None; }

    // 3. Evaluate — pure function, no side effects
    let decision = self.strategy.evaluate(state)?;

    // 4. Exposure check
    let total_exposure: f64 = self.open_positions.iter().map(|p| p.size_usdc).sum();
    if total_exposure >= self.max_exposure_usdc { return None; }

    // 5. Kelly sizing
    let size = (self.kelly_fraction * decision.confidence * self.balance)
        .min(self.max_position_usdc)
        .min(self.balance * 0.05);
    if size <= 0.0 { return None; }

    // 6. Apply slippage and fill
    let entry_price = decision.limit_price.as_f64() + self.slippage;
    self.balance -= size;
    self.open_positions.push(ActivePosition { ... });
    self.window_slots[slot] = true;

    Some(FillEvent { ... })
}
```

### Hot Path — Main Loop

```rust
// MarketState built ONCE per (asset, timeframe, tick)
let state = build_market_state(tick, timeframe, &window, &prices);

// Each instance evaluates independently
for instance in &mut instances {
    if let Some(fill) = instance.on_tick(&state) {
        info!(instance = %fill.label, asset = %fill.asset, ...);
    }
}
```

Window resolution:

```rust
for instance in &mut instances {
    let trades = instance.on_window_close(window_id, outcome, ts);
    for trade in &trades {
        info!(instance = %trade.label, result = ..., pnl = ...);
    }
}
```

**What disappears:**
- Global `position_opened` flag
- `strategies_attempted` bitmask
- Shared `RiskManager`
- Shared `PaperExecutor`
- `break` after first fill
- Cross-strategy correlation guard

**What stays:**
- `MarketState` built once, read by all
- Zero-alloc `evaluate()` (pure function)
- Per-instance window dedup
- Trend filter (global, applied to MarketState before instances see it)

### Backtest Parity

Backtest uses the exact same trait:

```rust
pub fn run_backtest(
    ticks: impl Iterator<Item = Tick>,
    instances: &mut [Box<dyn StrategyInstance>],
    enabled_assets: &[Asset],
    enabled_timeframes: &[Timeframe],
) -> BacktestResult
```

Same code path. Only difference: tick source (iterator vs WebSocket) and price source (model vs live).

Results are per-instance:

```rust
pub struct BacktestResult {
    pub instances: Vec<InstanceResult>,
    pub combined: CombinedResult,
}

pub struct InstanceResult {
    pub label: String,
    pub stats: InstanceStats,
    pub trades: Vec<TradeRecord>,
    pub equity_curve: Vec<f64>,
}
```

### Config

Each `[[bot.strategies]]` block becomes a fully independent instance:

```toml
[[bot.strategies]]
type = "early_directional"
label = "ED-tight"
balance = 125.0
max_position_usdc = 25.0
max_exposure_usdc = 100.0
kelly_fraction = 0.25
max_daily_loss = 50.0
max_entry_time_secs = 150
min_spot_magnitude = 0.002
max_entry_price = 0.53
```

Risk params move from global `[bot]` to per-strategy. Global `[bot]` keeps:
- `mode` (paper/backtest/live)
- `assets[]` (which assets/timeframes to scan)
- `trend_filter` (market filter, not strategy concern)

### Reporting

Periodic summary (every 5 min):

```
INSTANCE SUMMARY  instance=ED-tight  balance=$148.50  record=12W/3L (80%)  pnl=$+23.50
INSTANCE SUMMARY  instance=ED-loose  balance=$119.20  record=8W/5L (62%)   pnl=$-5.80
INSTANCE SUMMARY  instance=MC-tight  balance=$162.00  record=6W/1L (86%)   pnl=$+37.00
INSTANCE SUMMARY  instance=MC-loose  balance=$134.80  record=10W/4L (71%)  pnl=$+9.80
COMBINED          balance=$564.50    record=36W/13L (73%)  pnl=$+64.50
```

### Global Safety Net

One lightweight global circuit breaker sits above all instances:

```rust
let total_balance: f64 = instances.iter().map(|i| i.balance()).sum();
if total_balance < global_min_balance {
    warn!("global circuit breaker — total balance ${:.2} below minimum", total_balance);
    break; // stop all trading
}
```

Not a risk manager — just a hard stop if everything goes wrong at once.

## Files Affected

| File | Change |
|------|--------|
| `crates/pm-types/src/strategy.rs` | Add `StrategyInstance` trait, `InstanceStats`, `FillEvent` |
| `crates/pm-types/src/config.rs` | Move risk params into `StrategyConfig` variants, add `balance` field |
| `crates/pm-signal/src/` | New `instance.rs` — `ConcreteStrategyInstance` implementing the trait |
| `crates/pm-executor/src/backtest.rs` | Rewrite to use `&mut [Box<dyn StrategyInstance>]` |
| `crates/pm-executor/src/paper.rs` | Remove `PaperExecutor` — logic moves into instances |
| `crates/pm-risk/src/lib.rs` | Remove `RiskManager` — logic moves into instances |
| `src/paper.rs` | Simplify: build state → iterate instances → log events |
| `src/pbt_backtest.rs` | Use new backtest interface |
| `config/paper-aggressive.toml` | Add per-strategy balance/risk params |
| `config/production.toml` | Same |
| `tests/integration_pipeline.rs` | Rewrite to test instances directly |

## Migration Strategy

1. Build `ConcreteStrategyInstance` alongside existing code (no breaking changes)
2. Add `run_backtest_v2` using instances
3. Verify backtest results match existing `run_backtest`
4. Switch paper loop to use instances
5. Remove old `PaperExecutor`, `RiskManager` (or keep for reference)
6. Update integration tests

## Not In Scope

- Maker mode (Tier 1) — separate concern, works with any execution model
- Cross-venue arb (Tier 3) — separate concern
- TUI dashboard — separate concern, consumes `InstanceStats`
- Dynamic strategy hot-reload — future enhancement
