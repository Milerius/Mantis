# Strategy Architecture Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the shared executor/risk-manager architecture with independent `StrategyInstance`s that each own their balance, positions, and P&L — enabling hundreds of strategies to run simultaneously without blocking each other.

**Architecture:** Each TOML strategy config becomes a `ConcreteStrategyInstance` implementing the `StrategyInstance` trait. Instances are owned by value in the main loop. `MarketState` is built once per (asset, timeframe, tick) and passed read-only to all instances. Backtest and paper share the same trait.

**Tech Stack:** Rust, tokio, serde, tracing. Workspace at `/Users/milerius/Documents/Mantis/polymarket`. Build: `cargo build`, test: `cargo test --workspace`.

---

### Task 1: Add `StrategyInstance` trait, `InstanceStats`, `FillEvent` to pm-types

**Files:**
- Modify: `crates/pm-types/src/strategy.rs`
- Modify: `crates/pm-types/src/lib.rs`

- [ ] **Step 1: Add `InstanceStats` struct**

In `crates/pm-types/src/strategy.rs`, after the `EntryDecision` struct (around line 185), add:

```rust
// ─── InstanceStats ──────────────────────────────────────────────────────────

/// Per-instance session performance metrics.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct InstanceStats {
    pub wins: u32,
    pub losses: u32,
    pub realized_pnl: f64,
    pub biggest_win: f64,
    pub biggest_loss: f64,
}

impl InstanceStats {
    /// Record a resolved trade.
    pub fn record(&mut self, pnl: f64) {
        if pnl >= 0.0 {
            self.wins += 1;
            if pnl > self.biggest_win { self.biggest_win = pnl; }
        } else {
            self.losses += 1;
            if pnl < self.biggest_loss { self.biggest_loss = pnl; }
        }
        self.realized_pnl += pnl;
    }

    /// Win rate as a percentage (0–100).
    #[must_use]
    pub fn win_rate(&self) -> f64 {
        let total = self.wins + self.losses;
        if total == 0 { 0.0 } else { self.wins as f64 / total as f64 * 100.0 }
    }

    /// Format as "12W/3L (80%)"
    #[must_use]
    pub fn record_str(&self) -> alloc::string::String {
        alloc::format!("{}W/{}L ({:.0}%)", self.wins, self.losses, self.win_rate())
    }
}
```

- [ ] **Step 2: Add `FillEvent` struct**

After `InstanceStats`, add:

```rust
// ─── FillEvent ──────────────────────────────────────────────────────────────

/// Emitted by a `StrategyInstance` when it opens a position.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct FillEvent {
    pub label: StrategyLabel,
    pub asset: Asset,
    pub timeframe: Timeframe,
    pub side: Side,
    pub strategy_id: StrategyId,
    pub fill_price: f64,
    pub size_usdc: f64,
    pub confidence: f64,
    pub balance_after: f64,
}
```

- [ ] **Step 3: Add `StrategyInstance` trait**

After `FillEvent`, add:

```rust
// ─── StrategyInstance ───────────────────────────────────────────────────────

/// A fully self-contained strategy that owns its state.
/// Each instance has its own balance, positions, risk config, and P&L.
pub trait StrategyInstance: Send + Sync {
    /// Unique label for this instance (e.g. "ED-tight").
    fn label(&self) -> &str;

    /// Evaluate market state and optionally open a position.
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

Note: This trait requires `alloc` (for `Vec<TradeRecord>`). It should be gated behind `#[cfg(feature = "std")]` or `#[cfg(feature = "alloc")]` to match the crate's `no_std` policy.

- [ ] **Step 4: Export new types from lib.rs**

In `crates/pm-types/src/lib.rs`, update the strategy re-export line:

```rust
pub use strategy::{EntryDecision, FillEvent, InstanceStats, MarketState, StrategyId, StrategyLabel};
```

Also export the trait (std-only):

```rust
#[cfg(feature = "std")]
pub use strategy::StrategyInstance;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p pm-types`
Expected: All existing tests pass, no new tests yet.

- [ ] **Step 6: Commit**

```bash
git add crates/pm-types/src/strategy.rs crates/pm-types/src/lib.rs
git commit -m "feat(pm-types): add StrategyInstance trait, InstanceStats, FillEvent"
```

---

### Task 2: Update `StrategyConfig` with per-instance risk params

**Files:**
- Modify: `crates/pm-types/src/config.rs`

- [ ] **Step 1: Add per-instance risk fields to each strategy variant**

In `crates/pm-types/src/config.rs`, update the `StrategyConfig` enum. Add these fields to `EarlyDirectional` and `MomentumConfirmation` variants (after the existing fields):

```rust
    EarlyDirectional {
        #[serde(default)]
        label: String,
        max_entry_time_secs: u64,
        min_spot_magnitude: f64,
        max_entry_price: f64,
        // ── Per-instance risk params (optional, defaults applied) ──
        #[serde(default = "default_instance_balance")]
        balance: f64,
        #[serde(default = "default_instance_max_position")]
        max_position_usdc: f64,
        #[serde(default = "default_instance_max_exposure")]
        max_exposure_usdc: f64,
        #[serde(default = "default_instance_kelly")]
        kelly_fraction: f64,
        #[serde(default = "default_instance_max_daily_loss")]
        max_daily_loss: f64,
        #[serde(default = "default_instance_slippage")]
        slippage_bps: u32,
    },
```

Apply the same 6 new fields to `MomentumConfirmation`, `CompleteSetArb`, and `HedgeLock` variants.

- [ ] **Step 2: Add default functions**

After `default_strategies()`, add:

```rust
fn default_instance_balance() -> f64 { 125.0 }
fn default_instance_max_position() -> f64 { 25.0 }
fn default_instance_max_exposure() -> f64 { 100.0 }
fn default_instance_kelly() -> f64 { 0.25 }
fn default_instance_max_daily_loss() -> f64 { 50.0 }
fn default_instance_slippage() -> u32 { 10 }
```

- [ ] **Step 3: Update `default_strategies()` to include new fields**

Add the 6 new fields to each variant in `default_strategies()` with default values.

- [ ] **Step 4: Fix all test `StrategyConfig` constructions**

Search for `StrategyConfig::EarlyDirectional {` and `StrategyConfig::MomentumConfirmation {` in test code within `config.rs` and add the new fields. Tests use `..` or explicit values.

- [ ] **Step 5: Run tests**

Run: `cargo test -p pm-types`
Expected: All config deserialization tests pass. Existing TOML without the new fields deserializes with defaults.

- [ ] **Step 6: Commit**

```bash
git add crates/pm-types/src/config.rs
git commit -m "feat(config): add per-instance balance, risk, and slippage params to StrategyConfig"
```

---

### Task 3: Implement `ConcreteStrategyInstance`

**Files:**
- Create: `crates/pm-signal/src/instance.rs`
- Modify: `crates/pm-signal/src/lib.rs`
- Modify: `crates/pm-signal/Cargo.toml` (if needed for pm-types features)

This is the core of the redesign.

- [ ] **Step 1: Create `instance.rs` with struct definition**

Create `crates/pm-signal/src/instance.rs`:

```rust
//! Concrete implementation of [`StrategyInstance`] — a fully self-contained
//! strategy with its own balance, positions, risk, and P&L tracking.

extern crate alloc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use pm_types::{
    Asset, ContractPrice, EntryDecision, FillEvent, InstanceStats, MarketState,
    OpenPosition, Pnl, Side, StrategyId, StrategyInstance, StrategyLabel,
    Timeframe, TradeRecord, WindowId,
    trade::OrderReason,
};

use crate::AnyStrategy;
use crate::strategy_trait::Strategy;

/// Maximum number of (asset, timeframe) slots.
/// 4 assets × 8 timeframes = 32.
const MAX_SLOTS: usize = 32;

/// A fully independent strategy instance.
pub struct ConcreteStrategyInstance {
    // ── Identity ──
    label: String,
    strategy: AnyStrategy,

    // ── Balance & positions ──
    balance: f64,
    open_positions: Vec<ActivePosition>,
    trades: Vec<TradeRecord>,

    // ── Risk params ──
    max_position_usdc: f64,
    max_exposure_usdc: f64,
    kelly_fraction: f64,
    max_daily_loss: f64,
    slippage: f64,
    daily_pnl: f64,
    kill_switch: bool,

    // ── Per-window dedup ──
    /// Tracks which (asset, timeframe) window already has a position.
    /// Index: `asset.index() * Timeframe::COUNT + timeframe.index()`.
    /// Cleared when the window resolves.
    window_slots: [Option<WindowId>; MAX_SLOTS],

    // ── Stats ──
    stats: InstanceStats,

    // ── Internal counter ──
    next_order_id: u64,
}

/// Internal position tracking (not exported).
struct ActivePosition {
    pos: OpenPosition,
    slot: usize,
    strategy_id: StrategyId,
    label: StrategyLabel,
}
```

- [ ] **Step 2: Implement constructor**

```rust
impl ConcreteStrategyInstance {
    /// Build a new instance from a strategy and its risk config.
    #[must_use]
    pub fn new(
        label: String,
        strategy: AnyStrategy,
        balance: f64,
        max_position_usdc: f64,
        max_exposure_usdc: f64,
        kelly_fraction: f64,
        max_daily_loss: f64,
        slippage_bps: u32,
    ) -> Self {
        Self {
            label,
            strategy,
            balance,
            open_positions: Vec::new(),
            trades: Vec::new(),
            max_position_usdc,
            max_exposure_usdc,
            kelly_fraction,
            max_daily_loss,
            slippage: f64::from(slippage_bps) * 0.0001,
            daily_pnl: 0.0,
            kill_switch: false,
            window_slots: [None; MAX_SLOTS],
            stats: InstanceStats::default(),
            next_order_id: 1,
        }
    }
}
```

- [ ] **Step 3: Implement `StrategyInstance` trait — `on_tick`**

```rust
impl StrategyInstance for ConcreteStrategyInstance {
    fn label(&self) -> &str {
        &self.label
    }

    fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent> {
        // 1. Slot check
        let slot = state.asset.index() * Timeframe::COUNT + state.timeframe.index();
        if let Some(existing_wid) = self.window_slots[slot] {
            if existing_wid == state.window_id {
                return None; // already positioned in this window
            }
        }

        // 2. Kill switch
        if self.kill_switch {
            return None;
        }

        // 3. Evaluate — pure function
        let decision = self.strategy.evaluate(state)?;

        // 4. Exposure check
        let total_exposure: f64 = self.open_positions.iter().map(|p| p.pos.size_usdc).sum();
        if total_exposure >= self.max_exposure_usdc {
            return None;
        }

        // 5. Kelly sizing
        let raw_size = self.kelly_fraction * decision.confidence * self.balance;
        let size = raw_size
            .min(self.max_position_usdc)
            .min(self.balance * 0.05);
        if size <= 0.0 {
            return None;
        }

        // 6. Apply slippage and fill
        let raw_entry = decision.limit_price.as_f64() + self.slippage;
        let entry_clamped = raw_entry.clamp(0.01, 0.99);
        let avg_entry = ContractPrice::new(entry_clamped)?;

        // Deduct balance
        self.balance -= size;

        let pos = OpenPosition {
            window_id: state.window_id,
            asset: state.asset,
            side: decision.side,
            avg_entry,
            size_usdc: size,
            opened_at_ms: 0, // caller should set from tick timestamp
        };

        self.open_positions.push(ActivePosition {
            pos: pos.clone(),
            slot,
            strategy_id: decision.strategy_id,
            label: decision.label,
        });
        self.window_slots[slot] = Some(state.window_id);
        self.next_order_id += 1;

        Some(FillEvent {
            label: decision.label,
            asset: state.asset,
            timeframe: state.timeframe,
            side: decision.side,
            strategy_id: decision.strategy_id,
            fill_price: entry_clamped,
            size_usdc: size,
            confidence: decision.confidence,
            balance_after: self.balance,
        })
    }
```

- [ ] **Step 4: Implement `on_window_close`**

```rust
    fn on_window_close(
        &mut self,
        window_id: WindowId,
        outcome: Side,
        timestamp_ms: u64,
    ) -> Vec<TradeRecord> {
        let mut closed = Vec::new();
        let mut i = 0;
        while i < self.open_positions.len() {
            if self.open_positions[i].pos.window_id != window_id {
                i += 1;
                continue;
            }
            let ap = self.open_positions.swap_remove(i);
            let pos = ap.pos;
            let won = pos.side == outcome;

            let (exit_price_val, pnl_val) = if won {
                let entry = pos.avg_entry.as_f64();
                if entry > 0.0 {
                    let payout = pos.size_usdc / entry;
                    let pnl = payout - pos.size_usdc;
                    (1.0, pnl)
                } else {
                    (1.0, 0.0)
                }
            } else {
                (0.0, -pos.size_usdc)
            };

            self.balance += pos.size_usdc + pnl_val;
            self.daily_pnl += pnl_val;
            self.stats.record(pnl_val);

            // Check daily loss kill switch
            if self.daily_pnl < -self.max_daily_loss {
                self.kill_switch = true;
            }

            let exit_price = ContractPrice::new(exit_price_val)
                .unwrap_or(ContractPrice::new(0.5).expect("0.5 is valid"));

            closed.push(TradeRecord {
                window_id,
                asset: pos.asset,
                side: pos.side,
                entry_price: pos.avg_entry,
                exit_price,
                size_usdc: pos.size_usdc,
                pnl: Pnl::new(pnl_val).unwrap_or(Pnl::ZERO),
                opened_at_ms: pos.opened_at_ms,
                closed_at_ms: timestamp_ms,
                close_reason: OrderReason::ExpiryClose,
                strategy_id: ap.strategy_id,
            });

            // Clear the window slot
            self.window_slots[ap.slot] = None;
            // Don't increment i — swap_remove moved the last element here
        }
        closed
    }

    fn balance(&self) -> f64 {
        self.balance
    }

    fn stats(&self) -> &InstanceStats {
        &self.stats
    }
}
```

- [ ] **Step 5: Export from lib.rs**

In `crates/pm-signal/src/lib.rs`, add:

```rust
#[cfg(feature = "std")]
pub mod instance;
#[cfg(feature = "std")]
pub use instance::ConcreteStrategyInstance;
```

- [ ] **Step 6: Write tests**

At the bottom of `instance.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pm_types::{Asset, ContractPrice, Price, Side, Timeframe, WindowId};
    use crate::EarlyDirectional;

    fn make_instance() -> ConcreteStrategyInstance {
        let strategy = AnyStrategy::Early(
            EarlyDirectional::new(150, 0.002, 0.53)
        );
        ConcreteStrategyInstance::new(
            "ED-test".into(),
            strategy,
            500.0,  // balance
            25.0,   // max_position
            100.0,  // max_exposure
            0.25,   // kelly
            50.0,   // max_daily_loss
            10,     // slippage_bps
        )
    }

    fn make_state(elapsed: u64, magnitude: f64, ask: f64) -> MarketState {
        MarketState {
            asset: Asset::Btc,
            timeframe: Timeframe::Min15,
            window_id: WindowId::new(1),
            window_open_price: Price::new(95000.0).unwrap(),
            current_spot: Price::new(95000.0 * (1.0 + magnitude)).unwrap(),
            spot_magnitude: magnitude,
            spot_direction: Side::Up,
            time_elapsed_secs: elapsed,
            time_remaining_secs: 900 - elapsed,
            contract_ask_up: ContractPrice::new(ask),
            contract_ask_down: ContractPrice::new(1.0 - ask),
            contract_bid_up: ContractPrice::new(ask - 0.02),
            contract_bid_down: ContractPrice::new(1.0 - ask - 0.02),
            orderbook_imbalance: None,
        }
    }

    #[test]
    fn instance_opens_position_on_signal() {
        let mut inst = make_instance();
        let state = make_state(60, 0.005, 0.50);
        let fill = inst.on_tick(&state);
        assert!(fill.is_some(), "should fire on strong early move");
        assert!(inst.balance() < 500.0, "balance should decrease");
    }

    #[test]
    fn instance_blocks_duplicate_in_same_window() {
        let mut inst = make_instance();
        let state = make_state(60, 0.005, 0.50);
        let fill1 = inst.on_tick(&state);
        assert!(fill1.is_some());
        let fill2 = inst.on_tick(&state);
        assert!(fill2.is_none(), "should not open second position in same window");
    }

    #[test]
    fn instance_resolves_win_correctly() {
        let mut inst = make_instance();
        let state = make_state(60, 0.005, 0.50);
        let fill = inst.on_tick(&state).unwrap();

        let trades = inst.on_window_close(WindowId::new(1), Side::Up, 900_000);
        assert_eq!(trades.len(), 1);
        assert!(trades[0].pnl.as_f64() > 0.0, "should be profitable");
        assert_eq!(inst.stats().wins, 1);
        assert_eq!(inst.stats().losses, 0);
    }

    #[test]
    fn instance_resolves_loss_correctly() {
        let mut inst = make_instance();
        let state = make_state(60, 0.005, 0.50);
        inst.on_tick(&state);

        let trades = inst.on_window_close(WindowId::new(1), Side::Down, 900_000);
        assert_eq!(trades.len(), 1);
        assert!(trades[0].pnl.as_f64() < 0.0, "should be a loss");
        assert_eq!(inst.stats().losses, 1);
    }

    #[test]
    fn instance_kill_switch_on_daily_loss() {
        let mut inst = make_instance();
        // Open and lose many positions to trigger kill switch
        for i in 1..=20u64 {
            inst.window_slots = [None; MAX_SLOTS]; // reset slots
            let mut state = make_state(60, 0.005, 0.50);
            state.window_id = WindowId::new(i);
            if inst.on_tick(&state).is_some() {
                inst.on_window_close(WindowId::new(i), Side::Down, i * 900_000);
            }
        }
        // After enough losses, kill switch should be active
        let state = make_state(60, 0.005, 0.50);
        assert!(inst.on_tick(&state).is_none(), "kill switch should block trades");
    }

    #[test]
    fn different_windows_are_independent() {
        let mut inst = make_instance();

        let state1 = make_state(60, 0.005, 0.50);
        assert!(inst.on_tick(&state1).is_some());

        // Different window (different window_id)
        let mut state2 = make_state(60, 0.005, 0.50);
        state2.window_id = WindowId::new(2);
        state2.asset = Asset::Eth;
        assert!(inst.on_tick(&state2).is_some(), "different asset should be independent");
    }
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p pm-signal`
Expected: All new and existing tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/pm-signal/src/instance.rs crates/pm-signal/src/lib.rs
git commit -m "feat(pm-signal): implement ConcreteStrategyInstance with full test suite"
```

---

### Task 4: Add `build_instances_from_config` builder

**Files:**
- Modify: `crates/pm-signal/src/builder.rs`

- [ ] **Step 1: Add builder function**

In `crates/pm-signal/src/builder.rs`, add after `build_engine_from_config`:

```rust
use crate::instance::ConcreteStrategyInstance;
use pm_types::StrategyLabel;

/// Build a vec of independent [`ConcreteStrategyInstance`]s from config.
///
/// Each strategy config becomes a fully independent instance with its own
/// balance, positions, and risk parameters.
#[must_use]
pub fn build_instances_from_config(
    strategies: &[StrategyConfig],
) -> Vec<ConcreteStrategyInstance> {
    strategies
        .iter()
        .map(|s| {
            let (label_str, strategy, balance, max_pos, max_exp, kelly, max_loss, slippage) = match s {
                StrategyConfig::EarlyDirectional {
                    label, max_entry_time_secs, min_spot_magnitude, max_entry_price,
                    balance, max_position_usdc, max_exposure_usdc, kelly_fraction,
                    max_daily_loss, slippage_bps,
                } => {
                    let auto_label = if label.is_empty() {
                        format!("ED-{max_entry_price}")
                    } else {
                        label.clone()
                    };
                    let strat = AnyStrategy::Early(
                        EarlyDirectional::new(*max_entry_time_secs, *min_spot_magnitude, *max_entry_price)
                            .with_label(StrategyLabel::new(&auto_label))
                    );
                    (auto_label, strat, *balance, *max_position_usdc, *max_exposure_usdc, *kelly_fraction, *max_daily_loss, *slippage_bps)
                },
                StrategyConfig::MomentumConfirmation {
                    label, min_entry_time_secs, max_entry_time_secs, min_spot_magnitude, max_entry_price,
                    balance, max_position_usdc, max_exposure_usdc, kelly_fraction,
                    max_daily_loss, slippage_bps,
                } => {
                    let auto_label = if label.is_empty() {
                        format!("MC-{max_entry_price}")
                    } else {
                        label.clone()
                    };
                    let strat = AnyStrategy::Momentum(
                        MomentumConfirmation::new(*min_entry_time_secs, *max_entry_time_secs, *min_spot_magnitude, *max_entry_price)
                            .with_label(StrategyLabel::new(&auto_label))
                    );
                    (auto_label, strat, *balance, *max_position_usdc, *max_exposure_usdc, *kelly_fraction, *max_daily_loss, *slippage_bps)
                },
                StrategyConfig::CompleteSetArb { max_combined_cost, min_profit_per_share, .. } => {
                    let strat = AnyStrategy::Arb(CompleteSetArb::new(*max_combined_cost, *min_profit_per_share));
                    ("CSA".into(), strat, 125.0, 25.0, 100.0, 0.25, 50.0, 10)
                },
                StrategyConfig::HedgeLock { max_combined_cost, .. } => {
                    let strat = AnyStrategy::Hedge(HedgeLock::new(*max_combined_cost));
                    ("HL".into(), strat, 125.0, 25.0, 100.0, 0.25, 50.0, 10)
                },
            };

            ConcreteStrategyInstance::new(
                label_str, strategy, balance, max_pos, max_exp, kelly, max_loss, slippage,
            )
        })
        .collect()
}
```

- [ ] **Step 2: Export from lib.rs**

In `crates/pm-signal/src/lib.rs`, add:

```rust
#[cfg(feature = "std")]
pub use builder::build_instances_from_config;
```

- [ ] **Step 3: Add test**

In `builder.rs` tests:

```rust
    #[test]
    fn build_instances_from_defaults() {
        let defaults = default_strategies();
        let instances = build_instances_from_config(&defaults);
        assert_eq!(instances.len(), defaults.len());
        for inst in &instances {
            assert!(inst.balance() > 0.0);
            assert!(!inst.label().is_empty());
        }
    }
```

- [ ] **Step 4: Run tests and commit**

Run: `cargo test -p pm-signal`

```bash
git add crates/pm-signal/src/builder.rs crates/pm-signal/src/lib.rs
git commit -m "feat(pm-signal): add build_instances_from_config builder"
```

---

### Task 5: Rewrite backtest to use `StrategyInstance`

**Files:**
- Modify: `crates/pm-executor/src/backtest.rs`
- Modify: `crates/pm-executor/src/lib.rs`
- Modify: `src/pbt_backtest.rs`
- Modify: `src/backtest.rs`

- [ ] **Step 1: Add `run_backtest_v2` function**

In `crates/pm-executor/src/backtest.rs`, add a new function (keeping the old one for now):

```rust
use pm_types::{StrategyInstance, InstanceStats, FillEvent};

/// Backtest result with per-instance breakdown.
pub struct BacktestResultV2 {
    pub instances: Vec<InstanceResultV2>,
}

pub struct InstanceResultV2 {
    pub label: String,
    pub stats: InstanceStats,
    pub trades: Vec<TradeRecord>,
    pub final_balance: f64,
}

/// Run backtest using independent strategy instances.
///
/// Each instance manages its own balance, positions, and risk.
/// `MarketState` is built once per (asset, timeframe, tick) and
/// passed to all instances.
pub fn run_backtest_v2<P: ContractPriceProvider>(
    ticks: impl Iterator<Item = Tick>,
    instances: &mut [Box<dyn StrategyInstance>],
    price_provider: &P,
    enabled_assets: &[Asset],
    enabled_timeframes: &[Timeframe],
    trend_filter_config: Option<&TrendFilterConfig>,
) -> BacktestResultV2 {
    // Same window tracking as before
    const SLOTS: usize = Asset::COUNT * Timeframe::COUNT;
    let mut windows: [Option<Window>; SLOTS] = [None; SLOTS];
    let mut next_window_id: u64 = 1;

    let mut asset_enabled = [false; Asset::COUNT];
    for &a in enabled_assets {
        asset_enabled[a.index()] = true;
    }

    let tf_slots: Vec<TimeframeSlot> = enabled_timeframes
        .iter()
        .map(|&tf| TimeframeSlot {
            tf,
            duration_ms: tf.duration_secs() * 1_000,
            slot_offset: tf.index(),
        })
        .collect();

    // Optional EMA tracker for trend filter
    let mut ema_tracker = trend_filter_config
        .filter(|c| c.enabled)
        .map(|c| EmaTracker::new(c.fast_period, c.slow_period));
    let trend_filter = trend_filter_config.map(|c| TrendFilter {
        require_trend_alignment: c.enabled,
        min_trend_strength: c.min_trend_strength,
    });

    for tick in ticks {
        if !asset_enabled[tick.asset.index()] {
            continue;
        }

        // Update EMA
        if let Some(ref mut ema) = ema_tracker {
            ema.update(tick.asset, tick.price.as_f64());
        }

        for tfs in &tf_slots {
            let slot = tick.asset.index() * Timeframe::COUNT + tfs.slot_offset;
            let duration_ms = tfs.duration_ms;
            let window_open_ms = tick.timestamp_ms - (tick.timestamp_ms % duration_ms);
            let window_close_ms = window_open_ms + duration_ms;

            let need_new = windows[slot].is_none_or(|w| tick.timestamp_ms >= w.close_time_ms);

            if need_new {
                // Resolve old window across ALL instances
                if let Some(old_window) = windows[slot].take() {
                    let outcome = old_window.direction(tick.price);
                    for instance in instances.iter_mut() {
                        instance.on_window_close(old_window.id, outcome, tick.timestamp_ms);
                    }
                }

                let wid = WindowId::new(next_window_id);
                next_window_id += 1;
                windows[slot] = Some(Window {
                    id: wid,
                    asset: tick.asset,
                    timeframe: tfs.tf,
                    open_time_ms: window_open_ms,
                    close_time_ms: window_close_ms,
                    open_price: tick.price,
                });
            }

            let Some(window) = windows[slot] else { continue };

            let magnitude = window.magnitude(tick.price);
            let time_elapsed_secs = (tick.timestamp_ms.saturating_sub(window.open_time_ms)) / 1_000;
            let time_remaining_secs = window.time_remaining_secs(tick.timestamp_ms);

            let Some((ask_up, ask_down)) =
                price_provider.get_prices(tick.asset, tfs.tf, magnitude, time_elapsed_secs)
            else { continue };

            let spot_direction = window.direction(tick.price);

            let state = MarketState {
                asset: tick.asset,
                timeframe: tfs.tf,
                window_id: window.id,
                window_open_price: window.open_price,
                current_spot: tick.price,
                spot_magnitude: magnitude,
                spot_direction,
                time_elapsed_secs,
                time_remaining_secs,
                contract_ask_up: Some(ask_up),
                contract_ask_down: Some(ask_down),
                contract_bid_up: ContractPrice::new((ask_up.as_f64() - 0.02).clamp(0.0, 1.0)),
                contract_bid_down: ContractPrice::new((ask_down.as_f64() - 0.02).clamp(0.0, 1.0)),
                orderbook_imbalance: None,
            };

            // Each instance evaluates independently
            for instance in instances.iter_mut() {
                let _ = instance.on_tick(&state);
            }
        }
    }

    // Collect results
    BacktestResultV2 {
        instances: instances.iter().map(|inst| {
            InstanceResultV2 {
                label: inst.label().to_string(),
                stats: inst.stats().clone(),
                trades: Vec::new(), // TODO: expose trades from instance
                final_balance: inst.balance(),
            }
        }).collect(),
    }
}
```

- [ ] **Step 2: Update `src/pbt_backtest.rs` to use v2**

Replace the `run_backtest` call with `run_backtest_v2`:

```rust
use pm_signal::build_instances_from_config;

// Build instances instead of engine
let mut instances: Vec<Box<dyn pm_types::StrategyInstance>> =
    build_instances_from_config(&cfg.bot.strategies)
        .into_iter()
        .map(|i| Box::new(i) as Box<dyn pm_types::StrategyInstance>)
        .collect();

let result = run_backtest_v2(
    ticks.into_iter(),
    &mut instances,
    &price_provider,
    &enabled_assets,
    &enabled_timeframes,
    Some(&cfg.bot.trend_filter),
);

// Log per-instance results
for inst_result in &result.instances {
    info!(
        instance = %inst_result.label,
        balance = format!("${:.2}", inst_result.final_balance),
        record = %inst_result.stats.record_str(),
        pnl = format!("${:+.2}", inst_result.stats.realized_pnl),
        "instance result"
    );
}
```

- [ ] **Step 3: Run backtest to verify**

Run: `cargo run --release -- -c config/paper-aggressive.toml pbt-backtest`
Expected: Per-instance results printed. Compare aggregate with old results.

- [ ] **Step 4: Commit**

```bash
git add crates/pm-executor/src/backtest.rs src/pbt_backtest.rs
git commit -m "feat(backtest): add run_backtest_v2 with independent strategy instances"
```

---

### Task 6: Rewrite paper loop to use `StrategyInstance`

**Files:**
- Modify: `src/paper.rs`

This is the largest change. The `process_tick` function shrinks dramatically.

- [ ] **Step 1: Replace executor/risk with instances in main setup**

In the paper loop setup (around line 1000-1020 of `src/paper.rs`), replace:

```rust
// OLD: shared executor + risk
let mut executor = PaperExecutor::new(paper_config);
let mut risk = RiskManager::new(risk_config);
let engine = build_engine_from_config(&cfg.bot.strategies);
```

With:

```rust
// NEW: independent instances
use pm_signal::build_instances_from_config;

let mut instances: Vec<Box<dyn pm_types::StrategyInstance>> =
    build_instances_from_config(&cfg.bot.strategies)
        .into_iter()
        .map(|i| Box::new(i) as Box<dyn pm_types::StrategyInstance>)
        .collect();

info!(count = instances.len(), "strategy instances created");
for inst in &instances {
    info!(
        instance = %inst.label(),
        balance = format!("${:.2}", inst.balance()),
        "instance initialized"
    );
}
```

- [ ] **Step 2: Simplify `process_tick`**

Rewrite `process_tick` to:
1. Build `MarketState` once (existing code, unchanged)
2. Iterate instances, call `on_tick` on each
3. Log fill events

The function signature shrinks — remove `executor`, `risk`, `engine`, `stats`, `entry_timer`. Replace with `instances`.

- [ ] **Step 3: Simplify `handle_window_transition`**

Rewrite to iterate instances:

```rust
// Resolve old window
if let Some(old_lw) = live_windows[slot].take() {
    let outcome = old_lw.window.direction(tick.price);
    for instance in instances.iter_mut() {
        let trades = instance.on_window_close(old_lw.window.id, outcome, tick.timestamp_ms);
        for trade in &trades {
            let result = if trade.pnl.as_f64() >= 0.0 { "WIN" } else { "LOSS" };
            info!(
                instance = %instance.label(),
                asset = %trade.asset,
                timeframe = ?timeframe,
                result = result,
                pnl = format!("${:+.2}", trade.pnl.as_f64()),
                balance = format!("${:.2}", instance.balance()),
                record = %instance.stats().record_str(),
                "trade closed"
            );
        }
    }
    // ... open new window (unchanged)
}
```

- [ ] **Step 4: Update periodic summary**

Replace `SessionStats` with per-instance reporting:

```rust
// Every 5 minutes
let total_balance: f64 = instances.iter().map(|i| i.balance()).sum();
for instance in &instances {
    let s = instance.stats();
    info!(
        instance = %instance.label(),
        balance = format!("${:.2}", instance.balance()),
        record = %s.record_str(),
        pnl = format!("${:+.2}", s.realized_pnl),
        "instance summary"
    );
}
info!(
    total_balance = format!("${:.2}", total_balance),
    "combined summary"
);
```

- [ ] **Step 5: Remove `LiveWindow.position_opened` and `strategies_attempted`**

The `LiveWindow` struct simplifies to just:

```rust
struct LiveWindow {
    window: Window,
    pending_entry: Option<PendingEntry>, // keep if entry timing is still used
}
```

No more blocking flags. Each instance tracks its own window slots internally.

- [ ] **Step 6: Run paper trading and verify**

Run: `cargo build --release && RUST_LOG=info ./target/release/polymarket -c config/paper-aggressive.toml paper`

Expected: Per-instance fills and closures logged. All 4 strategies fire independently.

- [ ] **Step 7: Commit**

```bash
git add src/paper.rs
git commit -m "feat(paper): rewrite loop to use independent StrategyInstances"
```

---

### Task 7: Update configs and integration tests

**Files:**
- Modify: `config/paper-aggressive.toml`
- Modify: `config/production.toml`
- Modify: `tests/integration_pipeline.rs`

- [ ] **Step 1: Update paper-aggressive.toml**

Add per-instance risk params to each strategy:

```toml
[[bot.strategies]]
type = "early_directional"
label = "ED-tight"
balance = 125.0
max_position_usdc = 25.0
max_exposure_usdc = 100.0
kelly_fraction = 0.25
max_daily_loss = 50.0
slippage_bps = 10
max_entry_time_secs = 150
min_spot_magnitude = 0.002
max_entry_price = 0.53

[[bot.strategies]]
type = "early_directional"
label = "ED-loose"
balance = 125.0
max_position_usdc = 25.0
max_exposure_usdc = 100.0
kelly_fraction = 0.25
max_daily_loss = 50.0
slippage_bps = 10
max_entry_time_secs = 200
min_spot_magnitude = 0.001
max_entry_price = 0.58

[[bot.strategies]]
type = "momentum_confirmation"
label = "MC-tight"
balance = 125.0
max_position_usdc = 25.0
max_exposure_usdc = 100.0
kelly_fraction = 0.25
max_daily_loss = 50.0
slippage_bps = 10
min_entry_time_secs = 180
max_entry_time_secs = 480
min_spot_magnitude = 0.003
max_entry_price = 0.72

[[bot.strategies]]
type = "momentum_confirmation"
label = "MC-loose"
balance = 125.0
max_position_usdc = 25.0
max_exposure_usdc = 100.0
kelly_fraction = 0.25
max_daily_loss = 50.0
slippage_bps = 10
min_entry_time_secs = 120
max_entry_time_secs = 600
min_spot_magnitude = 0.001
max_entry_price = 0.65
```

- [ ] **Step 2: Update production.toml similarly**

- [ ] **Step 3: Rewrite integration tests**

Rewrite `tests/integration_pipeline.rs` to test `ConcreteStrategyInstance` directly instead of the old executor+risk pipeline.

- [ ] **Step 4: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add config/ tests/
git commit -m "feat: update configs and integration tests for independent strategy instances"
```

---

### Task 8: Cleanup — remove old shared executor/risk code

**Files:**
- Modify: `crates/pm-executor/src/paper.rs` (mark deprecated or remove)
- Modify: `crates/pm-risk/src/lib.rs` (mark deprecated or remove)
- Modify: `src/paper.rs` (remove unused imports)

- [ ] **Step 1: Remove `PaperExecutor` usage from paper.rs**

Remove all references to `PaperExecutor`, `RiskManager`, `SessionStats` (replaced by `InstanceStats`).

- [ ] **Step 2: Keep old code behind feature flag (optional)**

If you want to keep the old backtest path:
```rust
#[deprecated(note = "use run_backtest_v2 with StrategyInstance")]
pub fn run_backtest(...) { ... }
```

Or simply delete it.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace && cargo clippy --workspace`

- [ ] **Step 4: Commit**

```bash
git commit -am "refactor: remove deprecated PaperExecutor/RiskManager in favor of StrategyInstance"
```

---

## Self-Review

**Spec coverage:**
- StrategyInstance trait: Task 1
- ConcreteStrategyInstance internals: Task 3
- Per-instance balance/risk: Tasks 2, 3
- Hot path simplification: Task 6
- Backtest parity: Task 5
- Config changes: Tasks 2, 7
- Reporting: Task 6 (step 4)
- Global circuit breaker: Task 6 (step 4, in periodic summary)
- Migration strategy: Tasks built additively (v2 alongside old, then swap)

**No placeholders found.** All code blocks are complete.

**Type consistency verified:** `StrategyInstance`, `InstanceStats`, `FillEvent`, `ConcreteStrategyInstance`, `build_instances_from_config` — used consistently across all tasks.
