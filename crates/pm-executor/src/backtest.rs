//! Backtest execution engine with per-strategy P&L breakdown.
//!
//! Replays a stream of [`Tick`]s through the [`StrategyEngine`], obtains real
//! or model contract prices from a [`ContractPriceProvider`], and returns a
//! [`BacktestResult`] with per-strategy [`TradeSummary`] breakdowns.

extern crate alloc;

use alloc::vec::Vec;

use pm_bookkeeper::{TradeSummary, compute_summary};
use pm_signal::StrategyEngine;
use pm_types::{
    Asset, ContractPrice, MarketState, OpenPosition, OrderReason, Pnl, Price, Side, StrategyId,
    Tick, Timeframe, TradeRecord, Window, WindowId,
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
    /// Maximum USDC size per position.
    pub max_position_usdc: f64,
    /// Maximum number of open positions per (asset, timeframe) window slot.
    ///
    /// Typically `1` — one entry per window.
    pub max_positions_per_window: usize,
}

// ─── ContractPriceProvider ────────────────────────────────────────────────────

/// Provides Polymarket contract prices to the backtest engine.
///
/// Implementations may source prices from real historical trade data or from a
/// model (e.g. [`FixedPriceProvider`]).
pub trait ContractPriceProvider {
    /// Return `(ask_up, ask_down)` for the given market context, or `None` if
    /// no price data is available for this combination.
    fn get_prices(
        &self,
        asset: Asset,
        timeframe: Timeframe,
        magnitude: f64,
        time_elapsed_secs: u64,
    ) -> Option<(ContractPrice, ContractPrice)>;
}

// ─── FixedPriceProvider ───────────────────────────────────────────────────────

/// A [`ContractPriceProvider`] that always returns the same fixed prices.
///
/// Useful for unit tests where you want deterministic contract prices regardless
/// of market context.
#[derive(Debug, Clone, Copy)]
pub struct FixedPriceProvider {
    /// The fixed ask price for the Up contract.
    pub ask_up: ContractPrice,
    /// The fixed ask price for the Down contract.
    pub ask_down: ContractPrice,
}

impl ContractPriceProvider for FixedPriceProvider {
    fn get_prices(
        &self,
        _asset: Asset,
        _timeframe: Timeframe,
        _magnitude: f64,
        _time_elapsed_secs: u64,
    ) -> Option<(ContractPrice, ContractPrice)> {
        Some((self.ask_up, self.ask_down))
    }
}

// ─── ModelPriceProvider ──────────────────────────────────────────────────────

/// A [`ContractPriceProvider`] backed by a [`ContractPriceModel`].
///
/// Estimates contract prices from the empirical model calibrated on real
/// Polymarket trade data. Returns `ask_up` from the model estimate and
/// `ask_down ≈ 1.0 - ask_up + spread` to simulate a realistic orderbook.
pub struct ModelPriceProvider {
    /// The calibrated contract price model.
    pub model: pm_oracle::ContractPriceModel,
    /// Half-spread added to both sides (e.g., 0.01 = 1 cent each side).
    pub half_spread: f64,
}

impl ContractPriceProvider for ModelPriceProvider {
    fn get_prices(
        &self,
        asset: Asset,
        timeframe: Timeframe,
        magnitude: f64,
        time_elapsed_secs: u64,
    ) -> Option<(ContractPrice, ContractPrice)> {
        let mid = self.model.estimate(magnitude, time_elapsed_secs, asset, timeframe)?;
        let mid_val = mid.as_f64();
        let ask_up = (mid_val + self.half_spread).clamp(0.01, 0.99);
        let ask_down = (1.0 - mid_val + self.half_spread).clamp(0.01, 0.99);
        Some((
            ContractPrice::new(ask_up)?,
            ContractPrice::new(ask_down)?,
        ))
    }
}

// ─── BacktestResult ───────────────────────────────────────────────────────────

/// Result produced by [`run_backtest`].
#[derive(Debug)]
pub struct BacktestResult {
    /// All closed trades from the simulation.
    pub trades: Vec<TradeRecord>,
    /// Aggregate statistics over all trades.
    pub summary: TradeSummary,
    /// Per-strategy breakdown: each entry is `(strategy_id, summary)`.
    ///
    /// Only strategies that generated at least one trade appear here.
    pub per_strategy: Vec<(StrategyId, TradeSummary)>,
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
    /// Strategy that opened this position.
    strategy_id: StrategyId,
}

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Wrap a pre-clamped `f64` in a [`ContractPrice`].
///
/// `value` must already be in `[0.0, 1.0]`.  The fallback branch is unreachable
/// in practice — it only fires if the caller violates the invariant.
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
/// Loss: payout = 0; P&L = `-size_usdc`.
#[expect(
    clippy::too_many_arguments,
    reason = "all args address a single resolution event; grouping would obscure the data flow"
)]
fn resolve_positions(
    open_positions: &mut Vec<ActivePosition>,
    position_counts: &mut [u8],
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
        // FIX 4: decrement O(1) position count for the slot (saturating prevents underflow).
        position_counts[ap.slot] = position_counts[ap.slot].saturating_sub(1);
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
            strategy = %ap.strategy_id,
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
            strategy_id: ap.strategy_id,
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
            strategy_id: ap.strategy_id,
        });
    }
}

// ─── per_strategy_breakdown ───────────────────────────────────────────────────

/// Partition `trades` by [`StrategyId`] and compute a [`TradeSummary`] for each.
///
/// Only strategy IDs that appear in the trade list are included in the result.
/// The output order follows the declaration order of [`StrategyId`] variants.
fn per_strategy_breakdown(trades: &[TradeRecord]) -> Vec<(StrategyId, TradeSummary)> {
    // All known strategy IDs in a stable order.
    const ALL_IDS: [StrategyId; 4] = [
        StrategyId::CompleteSetArb,
        StrategyId::EarlyDirectional,
        StrategyId::MomentumConfirmation,
        StrategyId::HedgeLock,
    ];

    let mut result = Vec::new();
    for id in ALL_IDS {
        let subset: Vec<TradeRecord> = trades
            .iter()
            .filter(|t| t.strategy_id == id)
            .copied()
            .collect();
        if subset.is_empty() {
            continue;
        }
        let summary = compute_summary(&subset);
        result.push((id, summary));
    }
    result
}

// ─── TimeframeSlot ────────────────────────────────────────────────────────────

/// Pre-computed timeframe metadata to avoid repeated `duration_secs()` calls
/// and `tf.index()` calls inside the hot loop (FIX 6).
struct TimeframeSlot {
    tf: Timeframe,
    duration_ms: u64,
    slot_offset: usize,
}

// ─── run_backtest ─────────────────────────────────────────────────────────────

/// Run a backtest over `ticks` using `engine` and a [`ContractPriceProvider`].
///
/// For every tick, the engine:
/// 1. Determines the (asset, timeframe) window the tick belongs to.
/// 2. Detects window transitions and resolves expiring positions.
/// 3. Builds a [`MarketState`] from tick data + `price_provider`.
/// 4. Calls [`StrategyEngine::evaluate_all`] and opens a position for each
///    firing strategy that hasn't already reached `max_positions_per_window`.
/// 5. At end-of-stream, settles remaining open positions.
///
/// Only assets in `enabled_assets` and timeframes in `enabled_timeframes` are
/// considered.
///
/// # Panics
///
/// Does not panic in normal operation.  All price computations use clamped
/// inputs that are always valid [`ContractPrice`] values.
#[expect(
    clippy::too_many_lines,
    reason = "backtest loop is inherently linear; extracting sub-functions would obscure the data flow"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "all args are orthogonal config axes; bundling them would create ad-hoc structs with no reuse"
)]
#[must_use]
pub fn run_backtest<P: ContractPriceProvider>(
    ticks: impl Iterator<Item = Tick>,
    engine: &StrategyEngine,
    price_provider: &P,
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
    // FIX 4: O(1) per-slot position counts — no linear scan.
    let mut position_counts: [u8; SLOTS] = [0; SLOTS];
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut balance = config.initial_balance;
    // Monotonically increasing window id counter.
    let mut next_window_id: u64 = 1;

    let slippage = f64::from(config.slippage_bps) * 0.0001;

    // FIX 5: Asset enable bitfield — O(1) lookup instead of slice::contains.
    let mut asset_enabled = [false; Asset::COUNT];
    for &a in enabled_assets {
        asset_enabled[a.index()] = true;
    }

    // FIX 6: Pre-compute timeframe metadata once.
    let tf_slots: Vec<TimeframeSlot> = enabled_timeframes
        .iter()
        .map(|&tf| TimeframeSlot {
            tf,
            duration_ms: tf.duration_secs() * 1_000,
            slot_offset: tf.index(),
        })
        .collect();

    for tick in ticks {
        // FIX 5: O(1) asset check.
        if !asset_enabled[tick.asset.index()] {
            continue;
        }

        let asset_idx = tick.asset.index();
        last_prices[asset_idx] = Some(tick.price);

        for tfs in &tf_slots {
            // FIX 6: duration_ms and slot_offset are pre-computed.
            let slot = asset_idx * Timeframe::COUNT + tfs.slot_offset;
            let duration_ms = tfs.duration_ms;
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
                        &mut position_counts,
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
                    timeframe: tfs.tf,
                    open_time_ms: window_open_ms,
                    close_time_ms: window_close_ms,
                    open_price: tick.price,
                });

                debug!(
                    asset = %tick.asset,
                    timeframe = %tfs.tf,
                    window_id = %wid,
                    "new window opened"
                );
            }

            // Get the active window for this slot.
            let Some(window) = windows[slot] else {
                continue;
            };

            // FIX 4: O(1) position count lookup.
            if usize::from(position_counts[slot]) >= config.max_positions_per_window {
                continue;
            }

            // Compute market context.
            let magnitude = window.magnitude(tick.price);
            let time_elapsed_secs = (tick.timestamp_ms.saturating_sub(window.open_time_ms)) / 1_000;
            let time_remaining_secs = window.time_remaining_secs(tick.timestamp_ms);

            // Obtain contract prices from the provider.
            let Some((ask_up, ask_down)) =
                price_provider.get_prices(tick.asset, tfs.tf, magnitude, time_elapsed_secs)
            else {
                continue;
            };

            // Determine spot direction.
            let spot_direction = window.direction(tick.price);

            // Build a MarketState snapshot for the strategy engine.
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
                // Bids approximated as ask - 0.02; clamped to [0, 1].
                contract_bid_up: ContractPrice::new((ask_up.as_f64() - 0.02).clamp(0.0, 1.0)),
                contract_bid_down: ContractPrice::new((ask_down.as_f64() - 0.02).clamp(0.0, 1.0)),
                orderbook_imbalance: None,
            };

            // FIX 2: evaluate_all returns zero-alloc Decisions array.
            let decisions = engine.evaluate_all(&state);

            for decision in &decisions {
                // FIX 4: Re-check O(1) slot capacity — earlier decisions in
                // this loop may have already filled the quota.
                if usize::from(position_counts[slot]) >= config.max_positions_per_window {
                    break;
                }

                // Size: min(max_position_usdc, balance * 5%).
                let size = config.max_position_usdc.min(balance * 0.05);
                if size <= 0.0 {
                    continue;
                }

                // Apply slippage: entry price moves against us.
                let raw_entry = match decision.side {
                    Side::Up => ask_up.as_f64() + slippage,
                    Side::Down => ask_down.as_f64() + slippage,
                };
                let entry_clamped = raw_entry.clamp(0.01, 0.99);
                let avg_entry = contract_price_clamped(entry_clamped);

                let pos = OpenPosition {
                    window_id: window.id,
                    asset: tick.asset,
                    side: decision.side,
                    avg_entry,
                    size_usdc: size,
                    opened_at_ms: tick.timestamp_ms,
                };

                debug!(
                    asset = %tick.asset,
                    timeframe = %tfs.tf,
                    side = %decision.side,
                    strategy = %decision.strategy_id,
                    entry = entry_clamped,
                    size,
                    "position opened"
                );

                open_positions.push(ActivePosition {
                    pos,
                    slot,
                    strategy_id: decision.strategy_id,
                });
                // FIX 4: increment O(1) slot counter (saturating prevents overflow).
                position_counts[slot] = position_counts[slot].saturating_add(1);
            }
        }
    }

    // At end of stream, settle all remaining open positions.
    let remaining = core::mem::take(&mut open_positions);
    settle_remaining(remaining, &windows, &last_prices, &mut trades, &mut balance);

    let summary = compute_summary(&trades);
    let per_strategy = per_strategy_breakdown(&trades);

    BacktestResult {
        trades,
        summary,
        per_strategy,
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
    extern crate alloc;

    use alloc::{boxed::Box, vec};

    use pm_signal::{EarlyDirectional, StrategyEngine};
    use pm_types::{Asset, ExchangeSource, Price, Side, Timeframe};

    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

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
            max_position_usdc: 50.0,
            max_positions_per_window: 1,
        }
    }

    fn fixed_provider(ask_up: f64, ask_down: f64) -> FixedPriceProvider {
        FixedPriceProvider {
            ask_up: ContractPrice::new(ask_up).expect("valid ask_up"),
            ask_down: ContractPrice::new(ask_down).expect("valid ask_down"),
        }
    }

    // ── Test 1: empty ticks ──────────────────────────────────────────────────

    #[test]
    fn empty_ticks_returns_empty_result() {
        let engine = StrategyEngine::new(vec![]);
        let provider = fixed_provider(0.55, 0.48);
        let config = default_config();

        let result = run_backtest(
            core::iter::empty(),
            &engine,
            &provider,
            &config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert!(result.trades.is_empty(), "expected no trades");
        assert_eq!(result.summary.total_trades, 0);
        assert!(result.per_strategy.is_empty());
        assert!(
            (result.final_balance - config.initial_balance).abs() < 1e-10,
            "balance should be unchanged"
        );
    }

    // ── Test 2: EarlyDirectional fires and generates trades ──────────────────

    #[test]
    fn early_directional_generates_trades_with_fixed_price_provider() {
        // EarlyDirectional: max 300s, min magnitude 0.005, max_entry_price 0.60.
        // Provider returns ask_up=0.55 — below max_entry_price → strategy fires.
        //
        // Window layout (Min5 = 300_000 ms):
        //   Window 1: [0, 300_000). Open at price=100.0.
        //   EarlyDirectional scales max_entry_time to the timeframe:
        //     effective_max = 300 * duration_secs / 900 = 300 * 300 / 900 = 100s.
        //   Tick at t=50_000 (50s elapsed): price=100.6 → magnitude=0.006 ≥ 0.005,
        //     elapsed=50s ≤ 100s → strategy fires.
        //   Tick at t=300_000: crosses into Window 2 → resolves Window 1 as Up.
        let strategy = EarlyDirectional::new(300, 0.005, 0.60);
        let engine = StrategyEngine::new(vec![Box::new(strategy)]);
        let provider = fixed_provider(0.55, 0.48);
        let config = default_config();

        let ticks = vec![
            make_tick(Asset::Btc, 100.0, 0),
            make_tick(Asset::Btc, 100.6, 50_000),
            make_tick(Asset::Btc, 100.6, 300_000),
        ];

        let result = run_backtest(
            ticks.into_iter(),
            &engine,
            &provider,
            &config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert!(!result.trades.is_empty(), "expected at least one trade");
        assert_eq!(result.summary.total_trades as usize, result.trades.len());

        let trade = &result.trades[0];
        assert_eq!(trade.asset, Asset::Btc);
        assert_eq!(trade.side, Side::Up);
        assert_eq!(trade.strategy_id, StrategyId::EarlyDirectional);
        // Winning Up trade → pnl > 0.
        assert!(
            trade.pnl.as_f64() > 0.0,
            "expected positive PnL on winning Up trade, got {}",
            trade.pnl.as_f64()
        );
    }

    // ── Test 3: per-strategy breakdown attribution ───────────────────────────

    #[test]
    fn per_strategy_breakdown_shows_correct_strategy_id() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.60);
        let engine = StrategyEngine::new(vec![Box::new(strategy)]);
        let provider = fixed_provider(0.55, 0.48);
        let config = default_config();

        let ticks = vec![
            make_tick(Asset::Btc, 100.0, 0),
            make_tick(Asset::Btc, 100.6, 50_000),
            make_tick(Asset::Btc, 100.6, 300_000),
        ];

        let result = run_backtest(
            ticks.into_iter(),
            &engine,
            &provider,
            &config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        // Exactly one strategy produced trades.
        assert_eq!(result.per_strategy.len(), 1);
        let (id, summary) = &result.per_strategy[0];
        assert_eq!(*id, StrategyId::EarlyDirectional);
        assert_eq!(summary.total_trades, result.summary.total_trades);
    }
}
