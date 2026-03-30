//! Concrete implementation of [`StrategyInstance`] -- a fully self-contained
//! strategy with its own balance, positions, risk, and P&L tracking.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use pm_types::{
    Asset, ContractPrice, FillEvent, InstanceStats, MarketState, OpenPosition, Pnl, Side,
    StrategyId, StrategyInstance, StrategyLabel, Timeframe, TradeRecord, WindowId,
    trade::OrderReason,
};

use crate::AnyStrategy;

/// Maximum number of (asset, timeframe) slots.
/// 4 assets x 4 timeframes = 16.
const MAX_SLOTS: usize = Asset::COUNT * Timeframe::COUNT;

/// A fully independent strategy instance.
///
/// Owns its balance, open positions, risk parameters, and session stats.
/// Implements [`StrategyInstance`] so it can be driven by any loop (backtest,
/// paper, or live).
pub struct ConcreteStrategyInstance {
    // -- Identity --
    label: String,
    strategy: AnyStrategy,

    // -- Balance & positions --
    balance: f64,
    open_positions: Vec<ActivePosition>,
    #[expect(dead_code)] // accumulated for future export/inspection
    trades: Vec<TradeRecord>,

    // -- Risk params --
    max_position_usdc: f64,
    max_exposure_usdc: f64,
    kelly_fraction: f64,
    max_daily_loss: f64,
    slippage: f64,
    daily_pnl: f64,
    kill_switch: bool,

    // -- Per-window dedup --
    /// Tracks which (asset, timeframe) window already has a position.
    /// Index: `asset.index() * Timeframe::COUNT + timeframe.index()`.
    window_slots: [Option<WindowId>; MAX_SLOTS],

    // -- Stats --
    stats: InstanceStats,

    // -- Internal counter --
    next_order_id: u64,
}

/// Internal position tracking (not exported).
struct ActivePosition {
    pos: OpenPosition,
    slot: usize,
    strategy_id: StrategyId,
    #[expect(dead_code)] // stored for future trade-record enrichment
    label: StrategyLabel,
}

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

impl StrategyInstance for ConcreteStrategyInstance {
    fn label(&self) -> &str {
        &self.label
    }

    fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent> {
        // 1. Slot check -- one position per (asset, timeframe, window)
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

        // 3. Evaluate -- pure function
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
            pos,
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
            // Don't increment i -- swap_remove moved the last element here
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

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EarlyDirectional;
    use pm_types::{Asset, ContractPrice, Price, Side, Timeframe, WindowId};

    fn make_instance() -> ConcreteStrategyInstance {
        let strategy = AnyStrategy::Early(EarlyDirectional::new(150, 0.002, 0.53));
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
        assert!(
            fill2.is_none(),
            "should not open second position in same window"
        );
    }

    #[test]
    fn instance_resolves_win_correctly() {
        let mut inst = make_instance();
        let state = make_state(60, 0.005, 0.50);
        let _fill = inst.on_tick(&state).unwrap();

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
        assert!(
            inst.on_tick(&state).is_none(),
            "kill switch should block trades"
        );
    }

    #[test]
    fn different_windows_are_independent() {
        let mut inst = make_instance();

        let state1 = make_state(60, 0.005, 0.50);
        assert!(inst.on_tick(&state1).is_some());

        // Different window (different asset)
        let mut state2 = make_state(60, 0.005, 0.50);
        state2.window_id = WindowId::new(2);
        state2.asset = Asset::Eth;
        assert!(
            inst.on_tick(&state2).is_some(),
            "different asset should be independent"
        );
    }

    #[test]
    fn balance_restored_after_win() {
        let mut inst = make_instance();
        let initial = inst.balance();

        let state = make_state(60, 0.005, 0.50);
        let _fill = inst.on_tick(&state).unwrap();
        let after_open = inst.balance();
        assert!(after_open < initial);

        let trades = inst.on_window_close(WindowId::new(1), Side::Up, 900_000);
        let after_close = inst.balance();
        // Win at entry ~0.50 means payout = size / 0.50 = 2x size, so PnL = size.
        assert!(after_close > initial, "balance should grow after a win");
        assert!(trades[0].pnl.as_f64() > 0.0);
    }

    #[test]
    fn stats_accumulate_across_trades() {
        let mut inst = make_instance();

        // Win
        let state1 = make_state(60, 0.005, 0.50);
        inst.on_tick(&state1);
        inst.on_window_close(WindowId::new(1), Side::Up, 900_000);

        // Loss
        inst.window_slots = [None; MAX_SLOTS];
        let mut state2 = make_state(60, 0.005, 0.50);
        state2.window_id = WindowId::new(2);
        inst.on_tick(&state2);
        inst.on_window_close(WindowId::new(2), Side::Down, 1_800_000);

        assert_eq!(inst.stats().wins, 1);
        assert_eq!(inst.stats().losses, 1);
        assert!((inst.stats().win_rate() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn no_signal_no_fill() {
        let mut inst = make_instance();
        // Too late for EarlyDirectional (elapsed=200 > max_entry_time_secs=150)
        let state = make_state(200, 0.005, 0.50);
        assert!(
            inst.on_tick(&state).is_none(),
            "should not fire when too late"
        );
        assert!(
            (inst.balance() - 500.0).abs() < f64::EPSILON,
            "balance unchanged"
        );
    }
}
