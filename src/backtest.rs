//! Backtest subcommand: run the strategy engine over the test-set ticks and export results.

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_bookkeeper::{export_equity_curve, export_summary, export_trades_csv};
use pm_executor::{BacktestConfig, ModelPriceProvider, run_backtest};
use pm_oracle::HistoricalReplay;
use pm_signal::{
    CompleteSetArb, EarlyDirectional, HedgeLock, MomentumConfirmation, StrategyEngine,
};
use pm_types::{Asset, ExchangeSource, Timeframe, config::BotConfig};

use crate::calibrate::CalibrationResult;
use tracing::info;

/// Run a backtest over the test-set dates using the calibrated `table`.
///
/// A [`StrategyEngine`] is constructed with all four strategies using sensible
/// defaults.  Contract prices are derived from the calibrated [`LookupTable`]
/// via a [`LookupTablePriceProvider`].
///
/// Results are exported to `cfg.data.log_dir`:
/// - `trades.csv`    — full trade record
/// - `equity.csv`    — cumulative P&L curve
/// - `summary.json`  — aggregated statistics
///
/// # Errors
///
/// Returns an error if the cache files are missing, cannot be read, or if any
/// export fails.
pub fn run_backtest_cmd(cfg: &BotConfig, cal: CalibrationResult, test_dates: &[String]) -> Result<()> {
    let cache_dir = Path::new(&cfg.data.cache_dir);
    let log_dir = Path::new(&cfg.data.log_dir);

    let enabled_assets: Vec<Asset> = cfg
        .bot
        .assets
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.asset)
        .collect();

    // Collect all enabled timeframes.
    let mut enabled_timeframes: Vec<Timeframe> = Vec::new();
    for ac in &cfg.bot.assets {
        if !ac.enabled {
            continue;
        }
        for &tf in &ac.timeframes {
            if !enabled_timeframes.contains(&tf) {
                enabled_timeframes.push(tf);
            }
        }
    }

    info!(
        assets     = ?enabled_assets,
        timeframes = ?enabled_timeframes,
        days       = test_dates.len(),
        "loading test-set ticks"
    );

    let replay = HistoricalReplay::load(
        cache_dir,
        &enabled_assets,
        ExchangeSource::Binance,
        test_dates,
    )
    .context("failed to load test-set data")?;

    // Build the strategy engine with sensible defaults.
    // EarlyDirectional: enter within first 5 min if magnitude >= 0.5% and ask <= 0.65.
    // MomentumConfirmation: enter after 10 min if magnitude >= 1% and ask <= 0.70.
    // CompleteSetArb: fires when combined ask < $1.00 (spread captures the edge).
    // HedgeLock: fires to hedge a losing position.
    // Strategy defaults:
    // EarlyDirectional: enter within first 5 min if magnitude >= 0.5% and ask <= 0.65.
    // MomentumConfirmation: enter between 10–30 min if magnitude >= 1% and ask <= 0.70.
    // CompleteSetArb: fires when combined ask < $0.98 and profit-per-share > $0.02.
    // HedgeLock: fires when combined cost is below $0.25 to lock in a hedge.
    let engine = StrategyEngine::new(vec![
        Box::new(EarlyDirectional::new(300, 0.005, 0.65)),
        Box::new(MomentumConfirmation::new(600, 1_800, 0.01, 0.70)),
        Box::new(CompleteSetArb::new(0.98, 0.02)),
        Box::new(HedgeLock::new(0.25)),
    ]);

    // Use the calibrated contract price model as price provider.
    // Prices come from real Polymarket trade data paired with Binance spot data.
    // Half-spread of 0.01 (1 cent each side) simulates a realistic orderbook.
    let price_provider = ModelPriceProvider {
        model: cal.contract_model,
        half_spread: 0.01,
    };

    let bt_config = BacktestConfig {
        initial_balance: cfg.backtest.initial_balance,
        slippage_bps: cfg.backtest.slippage_bps,
        max_position_usdc: cfg.bot.max_position_usdc,
        max_positions_per_window: 1,
    };

    info!(
        initial_balance = cfg.backtest.initial_balance,
        slippage_bps = cfg.backtest.slippage_bps,
        max_position_usdc = cfg.bot.max_position_usdc,
        "running backtest"
    );

    let result = run_backtest(
        replay,
        &engine,
        &price_provider,
        &bt_config,
        &enabled_assets,
        &enabled_timeframes,
    );

    info!(
        total_trades = result.summary.total_trades,
        wins = result.summary.wins,
        losses = result.summary.losses,
        win_rate = result.summary.win_rate,
        total_pnl = result.summary.total_pnl,
        final_balance = result.final_balance,
        sharpe = result.summary.sharpe_ratio,
        profit_factor = result.summary.profit_factor,
        max_drawdown = result.summary.max_drawdown,
        "backtest complete"
    );

    // Log per-strategy breakdown.
    for (id, summary) in &result.per_strategy {
        info!(
            strategy = %id,
            trades = summary.total_trades,
            wins = summary.wins,
            win_rate = summary.win_rate,
            total_pnl = summary.total_pnl,
            "per-strategy summary"
        );
    }

    // Export results.
    export_trades_csv(&log_dir.join("trades.csv"), &result.trades)
        .context("failed to export trades.csv")?;
    export_equity_curve(&log_dir.join("equity.csv"), &result.trades)
        .context("failed to export equity.csv")?;
    export_summary(&log_dir.join("summary.json"), &result.summary)
        .context("failed to export summary.json")?;

    info!(log_dir = %log_dir.display(), "results exported");
    Ok(())
}
