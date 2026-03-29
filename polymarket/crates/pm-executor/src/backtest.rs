//! Backtest execution engine.
//!
//! Replays a stream of [`Tick`]s through the signal pipeline, simulates fills
//! with configurable slippage, and returns a [`BacktestResult`] containing all
//! closed [`TradeRecord`]s and a [`TradeSummary`].

use pm_bookkeeper::{TradeSummary, compute_summary};
use pm_signal::{FairValueEstimator, SignalEngine};
use pm_types::{
    Asset, ContractPrice, OpenPosition, OrderReason, Pnl, Price, Side, StrategyId, Tick,
    Timeframe, TradeRecord, Window, WindowId,
};
use tracing::debug;

// ─── BacktestConfig ───────────────────────────────────────────────────────────

/// Configuration for a backtest run.
#[derive(Debug, Clone)]
pub struct BacktestConfig {
    /// Starting USDC balance.
    pub initial_balance: f64,
    /// Slippage applied to entry price in basis points (1 bp = 0.0001).
    pub slippage_bps: u32,
    /// Minimum edge used when simulating the market price.
    ///
    /// The simulated market price is `fair_value - min_edge * 0.5`; the signal
    /// engine's own `min_edge` threshold controls whether a signal fires.  For
    /// signals to fire in backtest, this value should be larger than the engine's
    /// configured `min_edge`.
    pub min_edge: f64,
    /// Maximum USDC size per position.
    pub max_position_usdc: f64,
}

// ─── BacktestResult ───────────────────────────────────────────────────────────

/// Result produced by [`run_backtest`].
#[derive(Debug)]
pub struct BacktestResult {
    /// All closed trades from the simulation.
    pub trades: Vec<TradeRecord>,
    /// Aggregate statistics over all trades.
    pub summary: TradeSummary,
    /// Balance after all trades are settled.
    pub final_balance: f64,
}

// ─── Internal state ───────────────────────────────────────────────────────────

/// An in-flight position held by the backtest engine.
#[derive(Debug, Clone, Copy)]
struct ActivePosition {
    /// The underlying [`OpenPosition`] record.
    pos: OpenPosition,
    /// Index into the window slot table (`asset.index() * Timeframe::COUNT + tf.index()`).
    slot: usize,
}

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Wrap a pre-clamped `f64` in a [`ContractPrice`].
///
/// `value` must already be in `[0.0, 1.0]`.  The `unwrap_or_else` branch is
/// unreachable in practice (only fires if the invariant is violated).
#[expect(
    clippy::expect_used,
    reason = "fallback 0.5 is always a valid ContractPrice"
)]
fn contract_price_clamped(value: f64) -> ContractPrice {
    ContractPrice::new(value)
        .unwrap_or_else(|| ContractPrice::new(0.5).expect("0.5 is always a valid ContractPrice"))
}

// ─── resolve_positions ────────────────────────────────────────────────────────

/// Close all positions that match `window_id`, record [`TradeRecord`]s, and
/// update `balance`.
///
/// Win: payout = `size_usdc / entry_price`; P&L = `payout - size_usdc`.
/// Loss: payout = `0`; P&L = `-size_usdc`.
#[expect(
    clippy::too_many_arguments,
    reason = "all args are logically required to close a position"
)]
fn resolve_positions(
    open_positions: &mut Vec<ActivePosition>,
    trades: &mut Vec<TradeRecord>,
    balance: &mut f64,
    window_id: WindowId,
    outcome: Side,
    closed_at_ms: u64,
) {
    let mut i = 0;
    while i < open_positions.len() {
        if open_positions[i].pos.window_id != window_id {
            i += 1;
            continue;
        }

        let ap = open_positions.swap_remove(i);
        let pos = ap.pos;
        let entry = pos.avg_entry.as_f64();

        let (pnl_val, exit_price_val) = if pos.side == outcome {
            // Win: receive $1 per contract.
            if entry <= 0.0 {
                // Guard against degenerate entry price — treat as loss.
                (0.0_f64, 0.0_f64)
            } else {
                let num_contracts = pos.size_usdc / entry;
                let payout = num_contracts; // * $1.00
                (payout - pos.size_usdc, 1.0_f64)
            }
        } else {
            // Loss: forfeit the entire stake.
            (-pos.size_usdc, 0.0_f64)
        };

        *balance += pnl_val;

        let exit_price = contract_price_clamped(exit_price_val.clamp(0.0, 1.0));
        let pnl = Pnl::new(pnl_val).unwrap_or(Pnl::ZERO);

        debug!(
            window_id = %window_id,
            asset = %pos.asset,
            side = %pos.side,
            outcome = %outcome,
            pnl = pnl_val,
            "position resolved"
        );

        trades.push(TradeRecord {
            window_id,
            asset: pos.asset,
            side: pos.side,
            entry_price: pos.avg_entry,
            exit_price,
            size_usdc: pos.size_usdc,
            pnl,
            opened_at_ms: pos.opened_at_ms,
            closed_at_ms,
            close_reason: OrderReason::ExpiryClose,
            strategy_id: StrategyId::EarlyDirectional,
        });
    }
}

/// Settle any positions still open at end-of-stream using `last_prices`.
fn settle_remaining(
    remaining: Vec<ActivePosition>,
    windows: &[Option<Window>],
    last_prices: &[Option<Price>],
    trades: &mut Vec<TradeRecord>,
    balance: &mut f64,
) {
    for ap in remaining {
        let pos = ap.pos;
        let slot = ap.slot;
        let asset_idx = slot / Timeframe::COUNT;

        let Some(window) = windows[asset_idx * Timeframe::COUNT..][..Timeframe::COUNT]
            .iter()
            .find_map(|w| w.filter(|w| w.id == pos.window_id))
        else {
            continue;
        };

        let last_price = last_prices[asset_idx].unwrap_or(window.open_price);
        let outcome = window.direction(last_price);

        let entry = pos.avg_entry.as_f64();
        let (pnl_val, exit_price_val) = if pos.side == outcome && entry > 0.0 {
            let num_contracts = pos.size_usdc / entry;
            (num_contracts - pos.size_usdc, 1.0_f64)
        } else {
            (-pos.size_usdc, 0.0_f64)
        };

        *balance += pnl_val;

        let exit_price = contract_price_clamped(exit_price_val.clamp(0.0, 1.0));
        let pnl = Pnl::new(pnl_val).unwrap_or(Pnl::ZERO);

        trades.push(TradeRecord {
            window_id: pos.window_id,
            asset: pos.asset,
            side: pos.side,
            entry_price: pos.avg_entry,
            exit_price,
            size_usdc: pos.size_usdc,
            pnl,
            opened_at_ms: pos.opened_at_ms,
            closed_at_ms: window.close_time_ms,
            close_reason: OrderReason::ExpiryClose,
            strategy_id: StrategyId::EarlyDirectional,
        });
    }
}

// ─── run_backtest ─────────────────────────────────────────────────────────────

/// Run a backtest over `ticks` using `signal_engine` and `config`.
///
/// Only assets in `enabled_assets` and timeframes in `enabled_timeframes` are
/// considered.  Positions are opened when the signal engine fires, sized at
/// `min(max_position_usdc, balance * 0.05)`, with slippage applied.
///
/// # Panics
///
/// Does not panic in normal operation.  The internal market-price and
/// entry-price computations use clamped inputs that are always valid
/// [`ContractPrice`] values.
#[expect(
    clippy::too_many_lines,
    reason = "backtest loop is inherently linear; extracting sub-functions would obscure the data flow"
)]
#[must_use]
pub fn run_backtest<F: FairValueEstimator>(
    ticks: impl Iterator<Item = Tick>,
    signal_engine: &SignalEngine<F>,
    config: &BacktestConfig,
    enabled_assets: &[Asset],
    enabled_timeframes: &[Timeframe],
) -> BacktestResult {
    // Slot count: Asset::COUNT * Timeframe::COUNT
    const SLOTS: usize = Asset::COUNT * Timeframe::COUNT;

    // Active windows indexed by `asset.index() * Timeframe::COUNT + tf.index()`.
    let mut windows: [Option<Window>; SLOTS] = [None; SLOTS];
    // Last tick price per asset (used to determine outcome at stream end).
    let mut last_prices: [Option<Price>; Asset::COUNT] = [None; Asset::COUNT];

    let mut open_positions: Vec<ActivePosition> = Vec::new();
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut balance = config.initial_balance;
    // Monotonically increasing window id counter.
    let mut next_window_id: u64 = 1;

    let slippage = f64::from(config.slippage_bps) * 0.0001;

    for tick in ticks {
        // Only process enabled assets.
        if !enabled_assets.contains(&tick.asset) {
            continue;
        }

        let asset_idx = tick.asset.index();
        last_prices[asset_idx] = Some(tick.price);

        for &tf in enabled_timeframes {
            let slot = asset_idx * Timeframe::COUNT + tf.index();
            let duration_ms = tf.duration_secs() * 1_000;
            let window_open_ms = tick.timestamp_ms - (tick.timestamp_ms % duration_ms);
            let window_close_ms = window_open_ms + duration_ms;

            // Check whether the tick has crossed into a new window.
            let need_new_window =
                windows[slot].is_none_or(|w| tick.timestamp_ms >= w.close_time_ms);

            if need_new_window {
                // Resolve positions for the expiring window (if any).
                if let Some(old_window) = windows[slot].take() {
                    let outcome = old_window.direction(tick.price);
                    resolve_positions(
                        &mut open_positions,
                        &mut trades,
                        &mut balance,
                        old_window.id,
                        outcome,
                        tick.timestamp_ms,
                    );
                }

                // Open a new window.
                let wid = WindowId::new(next_window_id);
                next_window_id += 1;
                windows[slot] = Some(Window {
                    id: wid,
                    asset: tick.asset,
                    timeframe: tf,
                    open_time_ms: window_open_ms,
                    close_time_ms: window_close_ms,
                    open_price: tick.price,
                });

                debug!(
                    asset = %tick.asset,
                    timeframe = %tf,
                    window_id = %wid,
                    "new window opened"
                );
            }

            // Try to open a position if we don't already have one for this slot.
            let Some(window) = windows[slot] else {
                continue;
            };

            if open_positions.iter().any(|ap| ap.slot == slot) {
                continue;
            }

            // Compute fair value via the estimator.
            let magnitude = window.magnitude(tick.price);
            let time_remaining = window.time_remaining_secs(tick.timestamp_ms);
            let fair_value =
                signal_engine
                    .estimator()
                    .estimate(magnitude, time_remaining, tick.asset, tf);

            // Conservative simulated market price: fair_value shifted down by
            // `min_edge * 0.5`, clamped to [0.01, 0.99].  This represents a
            // market that prices the Up outcome slightly below our fair-value
            // estimate.  Using a config `min_edge` larger than the engine's
            // threshold ensures the resulting edge exceeds the engine's filter.
            let market_price_raw = (fair_value.as_f64() - config.min_edge * 0.5).clamp(0.01, 0.99);
            let market_price = contract_price_clamped(market_price_raw);

            let Some(signal) = signal_engine.evaluate(&tick, &window, market_price) else {
                continue;
            };

            // Size: min(max_position_usdc, balance * 5%)
            let size = config.max_position_usdc.min(balance * 0.05);
            if size <= 0.0 {
                continue;
            }

            // Apply slippage: entry price moves against us.
            let raw_entry = match signal.side {
                Side::Up => signal.market_price.as_f64() + slippage,
                Side::Down => signal.market_price.as_f64() - slippage,
            };
            let entry_clamped = raw_entry.clamp(0.01, 0.99);
            let avg_entry = contract_price_clamped(entry_clamped);

            let pos = OpenPosition {
                window_id: window.id,
                asset: tick.asset,
                side: signal.side,
                avg_entry,
                size_usdc: size,
                opened_at_ms: tick.timestamp_ms,
            };

            debug!(
                asset = %tick.asset,
                timeframe = %tf,
                side = %signal.side,
                entry = entry_clamped,
                size,
                "position opened"
            );

            open_positions.push(ActivePosition { pos, slot });
        }
    }

    // At end of stream, settle all remaining open positions.
    let remaining = core::mem::take(&mut open_positions);
    settle_remaining(remaining, &windows, &last_prices, &mut trades, &mut balance);

    let summary = compute_summary(&trades);

    BacktestResult {
        trades,
        summary,
        final_balance: balance,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test helpers use expect for conciseness"
)]
mod tests {
    use pm_signal::{LookupTable, SignalEngine};
    use pm_types::{Asset, ExchangeSource, Price, Timeframe};

    use super::*;

    fn make_tick(asset: Asset, price: f64, timestamp_ms: u64) -> Tick {
        Tick {
            asset,
            price: Price::new(price).expect("valid price"),
            timestamp_ms,
            source: ExchangeSource::Binance,
        }
    }

    fn default_config() -> BacktestConfig {
        BacktestConfig {
            initial_balance: 1_000.0,
            slippage_bps: 10,
            min_edge: 0.03,
            max_position_usdc: 50.0,
        }
    }

    // ── Test 1: empty ticks ──────────────────────────────────────────────────

    #[test]
    fn empty_ticks_returns_empty_result() {
        let table = LookupTable::new(1);
        let engine = SignalEngine::new(table, 0.03);
        let config = default_config();

        let result = run_backtest(
            core::iter::empty(),
            &engine,
            &config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert!(result.trades.is_empty(), "expected no trades");
        assert_eq!(result.summary.total_trades, 0);
        assert!(
            (result.final_balance - config.initial_balance).abs() < 1e-10,
            "balance should be unchanged"
        );
    }

    // ── Test 2: known-edge scenario with BTC ticks ───────────────────────────

    #[test]
    fn known_edge_scenario_generates_trades() {
        // Strategy:
        // - Config min_edge = 0.06 → simulated market_price = fair_value - 0.06 * 0.5
        //                                                    = 0.85 - 0.03 = 0.82
        // - Edge seen by engine = fair_value - market_price = 0.85 - 0.82 = 0.03
        // - Engine min_edge = 0.02 → 0.03 > 0.02 → signal fires ✓
        //
        // LookupTable cell for BTC/Min5 at magnitude ~0.4% and ~150 s remaining:
        // MAG_BOUNDARIES = [0.0, 0.001, 0.002, 0.003, 0.005, ...]
        //   0.004 is in (0.003, 0.005] → bucket 4
        // TIME_BOUNDARIES = [30, 60, 120, 180, ...]
        //   150 is in (120, 180] → bucket 3
        let mut table = LookupTable::new(1);
        let mag_b = LookupTable::mag_bucket(0.004);
        let time_b = LookupTable::time_bucket(150);
        table.set(Asset::Btc, Timeframe::Min5, mag_b, time_b, 0.85, 100);

        let engine = SignalEngine::new(table, 0.02);

        let config = BacktestConfig {
            initial_balance: 1_000.0,
            slippage_bps: 5,
            min_edge: 0.06,
            max_position_usdc: 50.0,
        };

        // Min5 = 300_000 ms per window.
        // Window 1: [0, 300_000). Open price = 100.0.
        // Move up ~0.4% → price = 100.4 at t=150_000 (mid-window).
        // Cross to Window 2 at t=300_000 → resolves Window 1 as Up.
        let open_price = 100.0_f64;
        let up_price = open_price * 1.004; // 0.4% up → magnitude 0.004

        let ticks = vec![
            make_tick(Asset::Btc, open_price, 0),
            make_tick(Asset::Btc, up_price, 150_000),
            make_tick(Asset::Btc, up_price, 300_000),
        ];

        let result = run_backtest(
            ticks.into_iter(),
            &engine,
            &config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert!(!result.trades.is_empty(), "expected at least one trade");
        assert_eq!(result.summary.total_trades as usize, result.trades.len());

        // The trade should be on BTC, Side::Up (price moved up).
        let trade = &result.trades[0];
        assert_eq!(trade.asset, Asset::Btc);
        assert_eq!(trade.side, Side::Up);
        // Winning Up trade → pnl > 0.
        assert!(
            trade.pnl.as_f64() > 0.0,
            "expected positive PnL on winning Up trade, got {}",
            trade.pnl.as_f64()
        );
    }
}
