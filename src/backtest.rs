//! Backtest subcommand: run the strategy engine over the test-set ticks and export results.

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_bookkeeper::{export_equity_curve, export_summary, export_trades_csv};
use pm_executor::{ModelPriceProvider, run_backtest_v2};
use pm_oracle::HistoricalReplay;
use pm_signal::build_instances_from_config;
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

    // Build independent strategy instances from TOML config.
    // Each instance owns its own balance, positions, and risk parameters.
    let mut instances: Vec<Box<dyn pm_types::StrategyInstance>> =
        build_instances_from_config(&cfg.bot.strategies)
            .into_iter()
            .map(|i| Box::new(i) as Box<dyn pm_types::StrategyInstance>)
            .collect();

    // Use the calibrated contract price model as price provider.
    // Prices come from real Polymarket trade data paired with Binance spot data.
    // Half-spread of 0.01 (1 cent each side) simulates a realistic orderbook.
    let price_provider = ModelPriceProvider {
        model: cal.contract_model,
        half_spread: 0.01,
    };

    info!(
        initial_balance = cfg.backtest.initial_balance,
        instances = instances.len(),
        "running backtest (v2)"
    );

    let result_v2 = run_backtest_v2(
        replay,
        &mut instances,
        &price_provider,
        &enabled_assets,
        &enabled_timeframes,
        Some(&cfg.bot.trend_filter),
    );

    // Log per-instance results.
    let mut all_trades: Vec<pm_types::TradeRecord> = Vec::new();
    for inst_result in &result_v2.instances {
        info!(
            instance = %inst_result.label,
            balance = format!("${:.2}", inst_result.final_balance),
            record = %inst_result.stats.record_str(),
            pnl = format!("${:+.2}", inst_result.stats.realized_pnl),
            trades = inst_result.trades.len(),
            "instance result"
        );
        all_trades.extend_from_slice(&inst_result.trades);
    }

    let total_balance: f64 = result_v2.instances.iter().map(|i| i.final_balance).sum();
    let total_pnl: f64 = result_v2.instances.iter().map(|i| i.stats.realized_pnl).sum();
    let total_wins: u32 = result_v2.instances.iter().map(|i| i.stats.wins).sum();
    let total_losses: u32 = result_v2.instances.iter().map(|i| i.stats.losses).sum();
    let total_trades_count = total_wins + total_losses;
    let win_rate = if total_trades_count > 0 {
        total_wins as f64 / total_trades_count as f64 * 100.0
    } else {
        0.0
    };

    info!(
        total_trades = total_trades_count,
        wins = total_wins,
        losses = total_losses,
        win_rate = format!("{win_rate:.1}%"),
        total_pnl = format!("${total_pnl:+.2}"),
        total_balance = format!("${total_balance:.2}"),
        "backtest complete (v2)"
    );

    // Export results.
    let combined_summary = pm_bookkeeper::compute_summary(&all_trades);

    export_trades_csv(&log_dir.join("trades.csv"), &all_trades)
        .context("failed to export trades.csv")?;
    export_equity_curve(&log_dir.join("equity.csv"), &all_trades)
        .context("failed to export equity.csv")?;
    export_summary(&log_dir.join("summary.json"), &combined_summary)
        .context("failed to export summary.json")?;

    info!(log_dir = %log_dir.display(), "results exported");
    Ok(())
}
