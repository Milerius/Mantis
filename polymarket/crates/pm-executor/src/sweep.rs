//! Parameter sweep: run the backtest across many parameter combinations.
//!
//! Pre-loads all tick data into a [`Vec<Tick>`], then iterates every combination
//! of [`EarlyDirectional`] and [`MomentumConfirmation`] parameters.  [`CompleteSetArb`]
//! and [`HedgeLock`] are held at fixed defaults for every run.
//!
//! FIX 1: All parameter combinations are run in parallel via `rayon`.
//! FIX 3: Uses [`AnyStrategy`] enum dispatch — no `Box<dyn Strategy>` on the hot path.
//!
//! Results are sorted by total P&L descending so that callers can immediately
//! inspect the best-performing configurations.

extern crate alloc;

use alloc::vec::Vec;

use rayon::prelude::*;

use pm_signal::{
    AnyStrategy, CompleteSetArb, EarlyDirectional, HedgeLock, MomentumConfirmation, StrategyEngine,
};
use pm_types::{Asset, Tick, Timeframe};

use crate::backtest::{BacktestConfig, ContractPriceProvider, run_backtest};

// ─── SweepConfig ──────────────────────────────────────────────────────────────

/// Parameter combinations to sweep.
///
/// The sweep iterates the full Cartesian product of all `early_*` and
/// `momentum_*` value lists, so keep the lists short to avoid combinatorial
/// explosion.  [`CompleteSetArb`] and [`HedgeLock`] are fixed at sensible
/// defaults for every run.
#[derive(Debug, Clone)]
pub struct SweepConfig {
    /// `EarlyDirectional`: `max_entry_time_secs` values to try.
    pub early_max_times: Vec<u64>,
    /// `EarlyDirectional`: `min_spot_magnitude` values to try.
    pub early_min_magnitudes: Vec<f64>,
    /// `EarlyDirectional`: `max_entry_price` values to try.
    pub early_max_prices: Vec<f64>,
    /// `MomentumConfirmation`: `min_entry_time_secs` values to try.
    pub momentum_min_times: Vec<u64>,
    /// `MomentumConfirmation`: `max_entry_time_secs` values to try.
    pub momentum_max_times: Vec<u64>,
    /// `MomentumConfirmation`: `min_spot_magnitude` values to try.
    pub momentum_min_mags: Vec<f64>,
    /// `MomentumConfirmation`: `max_entry_price` values to try.
    pub momentum_max_prices: Vec<f64>,
}

// ─── SweepResult ──────────────────────────────────────────────────────────────

/// Result of one parameter combination run.
#[derive(Debug, Clone)]
pub struct SweepResult {
    /// `EarlyDirectional` — `max_entry_time_secs` used in this run.
    pub early_max_time: u64,
    /// `EarlyDirectional` — `min_spot_magnitude` used in this run.
    pub early_min_mag: f64,
    /// `EarlyDirectional` — `max_entry_price` used in this run.
    pub early_max_price: f64,
    /// `MomentumConfirmation` — `min_entry_time_secs` used in this run.
    pub momentum_min_time: u64,
    /// `MomentumConfirmation` — `max_entry_time_secs` used in this run.
    pub momentum_max_time: u64,
    /// `MomentumConfirmation` — `min_spot_magnitude` used in this run.
    pub momentum_min_mag: f64,
    /// `MomentumConfirmation` — `max_entry_price` used in this run.
    pub momentum_max_price: f64,
    /// Aggregate P&L over all trades in this run.
    pub total_pnl: f64,
    /// Fraction of winning trades (`wins / total_trades`).
    pub win_rate: f64,
    /// Total number of closed trades.
    pub total_trades: u32,
    /// Annualised Sharpe ratio.
    pub sharpe: f64,
    /// Gross profit / gross loss (`0.0` when there are no losses).
    pub profit_factor: f64,
}

// ─── Fixed arb / hedge defaults ───────────────────────────────────────────────

/// Fixed `max_combined_cost` used for [`CompleteSetArb`] in every sweep run.
const ARB_MAX_COMBINED: f64 = 0.98;
/// Fixed `min_profit_per_share` used for [`CompleteSetArb`] in every sweep run.
const ARB_MIN_PROFIT: f64 = 0.02;
/// Fixed `max_combined_cost` used for [`HedgeLock`] in every sweep run.
const HEDGE_MAX_COMBINED: f64 = 0.25;

// ─── Combo ────────────────────────────────────────────────────────────────────

/// One fully-specified parameter combination produced by the Cartesian product.
#[derive(Debug, Clone, Copy)]
struct Combo {
    early_max_time: u64,
    early_min_mag: f64,
    early_max_price: f64,
    mom_min_time: u64,
    mom_max_time: u64,
    mom_min_mag: f64,
    mom_max_price: f64,
}

// ─── run_sweep ────────────────────────────────────────────────────────────────

/// Run the backtest across all parameter combinations defined by `sweep_config`.
///
/// `ticks` is a pre-loaded, time-sorted slice of [`Tick`]s.  For each
/// combination the function calls [`run_backtest`] with a freshly constructed
/// [`StrategyEngine`] by passing `ticks.iter().copied()`.
///
/// **FIX 1**: All combinations are evaluated in parallel using `rayon`.
/// **FIX 3**: Strategies use [`AnyStrategy`] enum dispatch — no vtable overhead.
///
/// The returned [`Vec<SweepResult>`] is sorted by `total_pnl` **descending**
/// (best configuration first).
///
/// # Returns
///
/// An empty vec when either `ticks` is empty or any parameter list in
/// `sweep_config` is empty (no combinations exist).
#[expect(
    clippy::too_many_arguments,
    reason = "all args are orthogonal axes required by the underlying backtest; bundling into an ad-hoc struct would add no clarity"
)]
#[must_use]
pub fn run_sweep<P: ContractPriceProvider + Sync>(
    ticks: &[Tick],
    price_provider: &P,
    base_config: &BacktestConfig,
    sweep_config: &SweepConfig,
    enabled_assets: &[Asset],
    enabled_timeframes: &[Timeframe],
) -> Vec<SweepResult> {
    // ── Build Cartesian product of all parameter combinations ──────────────
    let mut combos: Vec<Combo> = Vec::new();

    for &early_max_time in &sweep_config.early_max_times {
        for &early_min_mag in &sweep_config.early_min_magnitudes {
            for &early_max_price in &sweep_config.early_max_prices {
                for &mom_min_time in &sweep_config.momentum_min_times {
                    for &mom_max_time in &sweep_config.momentum_max_times {
                        // Skip degenerate windows where min >= max.
                        if mom_min_time >= mom_max_time {
                            continue;
                        }
                        for &mom_min_mag in &sweep_config.momentum_min_mags {
                            for &mom_max_price in &sweep_config.momentum_max_prices {
                                combos.push(Combo {
                                    early_max_time,
                                    early_min_mag,
                                    early_max_price,
                                    mom_min_time,
                                    mom_max_time,
                                    mom_min_mag,
                                    mom_max_price,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // ── FIX 1: Evaluate all combos in parallel with rayon ─────────────────
    let mut results: Vec<SweepResult> = combos
        .into_par_iter()
        .map(|combo| {
            // FIX 3: AnyStrategy enum dispatch — zero vtable cost.
            let engine = StrategyEngine::from_any(alloc::vec![
                AnyStrategy::Early(EarlyDirectional::new(
                    combo.early_max_time,
                    combo.early_min_mag,
                    combo.early_max_price,
                )),
                AnyStrategy::Momentum(MomentumConfirmation::new(
                    combo.mom_min_time,
                    combo.mom_max_time,
                    combo.mom_min_mag,
                    combo.mom_max_price,
                )),
                AnyStrategy::Arb(CompleteSetArb::new(ARB_MAX_COMBINED, ARB_MIN_PROFIT)),
                AnyStrategy::Hedge(HedgeLock::new(HEDGE_MAX_COMBINED)),
            ]);

            let bt = run_backtest(
                ticks.iter().copied(),
                &engine,
                price_provider,
                base_config,
                enabled_assets,
                enabled_timeframes,
            );

            SweepResult {
                early_max_time: combo.early_max_time,
                early_min_mag: combo.early_min_mag,
                early_max_price: combo.early_max_price,
                momentum_min_time: combo.mom_min_time,
                momentum_max_time: combo.mom_max_time,
                momentum_min_mag: combo.mom_min_mag,
                momentum_max_price: combo.mom_max_price,
                total_pnl: bt.summary.total_pnl,
                win_rate: bt.summary.win_rate,
                total_trades: bt.summary.total_trades,
                sharpe: bt.summary.sharpe_ratio,
                profit_factor: bt.summary.profit_factor,
            }
        })
        .collect();

    // Sort best-to-worst by total P&L.
    results.sort_by(|a, b| {
        b.total_pnl
            .partial_cmp(&a.total_pnl)
            .unwrap_or(core::cmp::Ordering::Equal)
    });

    results
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test helpers use expect for conciseness"
)]
mod tests {
    extern crate alloc;

    use pm_types::{Asset, ContractPrice, ExchangeSource, Price, Timeframe};

    use super::*;
    use crate::backtest::FixedPriceProvider;

    fn make_tick(asset: Asset, price: f64, timestamp_ms: u64) -> Tick {
        Tick {
            asset,
            price: Price::new(price).expect("valid price"),
            timestamp_ms,
            source: ExchangeSource::Binance,
        }
    }

    fn default_bt_config() -> BacktestConfig {
        BacktestConfig {
            initial_balance: 1_000.0,
            slippage_bps: 10,
            max_position_usdc: 50.0,
            max_positions_per_window: 1,
        }
    }

    fn fixed_provider() -> FixedPriceProvider {
        FixedPriceProvider {
            ask_up: ContractPrice::new(0.55).expect("valid"),
            ask_down: ContractPrice::new(0.48).expect("valid"),
        }
    }

    // ── 2×2 sweep produces 4 results, sorted correctly ───────────────────────

    #[test]
    fn sweep_2x2_produces_four_results_sorted_by_pnl() {
        // A minimal tick stream: 3 ticks creating one Min5 window (300 000 ms)
        // with a clear Up move.  EarlyDirectional will fire on some configs.
        let ticks = alloc::vec![
            make_tick(Asset::Btc, 100.0, 0),
            make_tick(Asset::Btc, 100.6, 150_000),
            make_tick(Asset::Btc, 100.6, 300_000),
        ];

        let sweep_config = SweepConfig {
            // Two values for early_max_time → 2 early configs.
            early_max_times: alloc::vec![120, 300],
            // Only one magnitude and price so only early_max_times varies here.
            early_min_magnitudes: alloc::vec![0.005],
            early_max_prices: alloc::vec![0.60],
            // Two momentum window sizes; both must have min < max.
            momentum_min_times: alloc::vec![200],
            momentum_max_times: alloc::vec![600],
            momentum_min_mags: alloc::vec![0.005],
            momentum_max_prices: alloc::vec![0.70],
        };

        let provider = fixed_provider();
        let bt_config = default_bt_config();

        let results = run_sweep(
            &ticks,
            &provider,
            &bt_config,
            &sweep_config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        // 2 early_max_times × 1 mag × 1 price × 1 mom_min × 1 mom_max × 1 mom_mag × 1 mom_price
        assert_eq!(results.len(), 2, "expected 2 results");

        // Verify sorted descending by total_pnl.
        for window in results.windows(2) {
            assert!(
                window[0].total_pnl >= window[1].total_pnl,
                "results must be sorted descending by pnl: {} < {}",
                window[0].total_pnl,
                window[1].total_pnl
            );
        }
    }

    // ── degenerate momentum window (min >= max) is skipped ───────────────────

    #[test]
    fn sweep_skips_degenerate_momentum_window() {
        let ticks = alloc::vec![make_tick(Asset::Btc, 100.0, 0)];

        let sweep_config = SweepConfig {
            early_max_times: alloc::vec![120],
            early_min_magnitudes: alloc::vec![0.005],
            early_max_prices: alloc::vec![0.60],
            // min == max → degenerate, should be skipped.
            momentum_min_times: alloc::vec![300],
            momentum_max_times: alloc::vec![300],
            momentum_min_mags: alloc::vec![0.005],
            momentum_max_prices: alloc::vec![0.70],
        };

        let provider = fixed_provider();
        let bt_config = default_bt_config();

        let results = run_sweep(
            &ticks,
            &provider,
            &bt_config,
            &sweep_config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert!(results.is_empty(), "degenerate windows must be skipped");
    }

    // ── empty ticks produces results with zero trades ────────────────────────

    #[test]
    fn sweep_empty_ticks_produces_zero_trade_results() {
        let sweep_config = SweepConfig {
            early_max_times: alloc::vec![120, 300],
            early_min_magnitudes: alloc::vec![0.005],
            early_max_prices: alloc::vec![0.60],
            momentum_min_times: alloc::vec![200],
            momentum_max_times: alloc::vec![600],
            momentum_min_mags: alloc::vec![0.005],
            momentum_max_prices: alloc::vec![0.70],
        };

        let provider = fixed_provider();
        let bt_config = default_bt_config();

        let results = run_sweep(
            &[],
            &provider,
            &bt_config,
            &sweep_config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert_eq!(
            results.len(),
            2,
            "still 2 combinations even with empty ticks"
        );
        for r in &results {
            assert_eq!(r.total_trades, 0);
            assert_eq!(r.total_pnl, 0.0);
        }
    }

    // ── parameter values are correctly propagated to results ─────────────────

    #[test]
    fn sweep_result_carries_correct_params() {
        let ticks = alloc::vec![make_tick(Asset::Btc, 100.0, 0)];

        let sweep_config = SweepConfig {
            early_max_times: alloc::vec![42],
            early_min_magnitudes: alloc::vec![0.007],
            early_max_prices: alloc::vec![0.62],
            momentum_min_times: alloc::vec![100],
            momentum_max_times: alloc::vec![500],
            momentum_min_mags: alloc::vec![0.009],
            momentum_max_prices: alloc::vec![0.68],
        };

        let provider = fixed_provider();
        let bt_config = default_bt_config();

        let results = run_sweep(
            &ticks,
            &provider,
            &bt_config,
            &sweep_config,
            &[Asset::Btc],
            &[Timeframe::Min5],
        );

        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.early_max_time, 42);
        assert!((r.early_min_mag - 0.007).abs() < 1e-12);
        assert!((r.early_max_price - 0.62).abs() < 1e-12);
        assert_eq!(r.momentum_min_time, 100);
        assert_eq!(r.momentum_max_time, 500);
        assert!((r.momentum_min_mag - 0.009).abs() < 1e-12);
        assert!((r.momentum_max_price - 0.68).abs() < 1e-12);
    }
}
