//! Backtest subcommand: run the signal engine over the test-set ticks and export results.

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_bookkeeper::{export_equity_curve, export_summary, export_trades_csv};
use pm_executor::{BacktestConfig, run_backtest};
use pm_oracle::HistoricalReplay;
use pm_signal::{LookupTable, SignalEngine};
use pm_types::{Asset, ExchangeSource, Timeframe, config::BotConfig};
use tracing::info;

/// Run a backtest over the test-set dates using the calibrated `table`.
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
pub fn run_backtest_cmd(cfg: &BotConfig, table: LookupTable, test_dates: &[String]) -> Result<()> {
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

    let engine = SignalEngine::new(table, cfg.bot.min_edge);

    let bt_config = BacktestConfig {
        initial_balance: cfg.backtest.initial_balance,
        slippage_bps: cfg.backtest.slippage_bps,
        min_edge: cfg.bot.min_edge,
        max_position_usdc: cfg.bot.max_position_usdc,
    };

    info!(
        initial_balance = cfg.backtest.initial_balance,
        slippage_bps = cfg.backtest.slippage_bps,
        min_edge = cfg.bot.min_edge,
        "running backtest"
    );

    let result = run_backtest(
        replay,
        &engine,
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
