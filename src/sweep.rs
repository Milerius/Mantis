//! Sweep subcommand: run the parameter sweep and print the top 10 results.
//!
//! Usage:
//! ```text
//! polymarket sweep -c config/test-week.toml
//! ```

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_executor::{BacktestConfig, FixedPriceProvider, SweepConfig, run_sweep};
use pm_oracle::HistoricalReplay;
use pm_types::{Asset, ContractPrice, ExchangeSource, Timeframe, config::BotConfig};
use tracing::info;

use crate::calibrate;

/// Run the parameter sweep and print the top 10 configurations.
///
/// Uses the test-set dates (40 % split) for evaluation so that results are
/// not biased by the training data used in calibration.
///
/// # Errors
///
/// Returns an error if data loading or calibration fails.
#[expect(
    clippy::too_many_lines,
    reason = "sweep command is a linear pipeline of config, load, run, print; splitting would obscure intent"
)]
pub fn run_sweep_cmd(cfg: &BotConfig) -> Result<()> {
    let cache_dir = Path::new(&cfg.data.cache_dir);

    let enabled_assets: Vec<Asset> = cfg
        .bot
        .assets
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.asset)
        .collect();

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

    let (_train_dates, test_dates) = calibrate::split_dates(cfg)?;

    info!(
        assets     = ?enabled_assets,
        timeframes = ?enabled_timeframes,
        days       = test_dates.len(),
        "loading sweep ticks"
    );

    let replay = HistoricalReplay::load(
        cache_dir,
        &enabled_assets,
        ExchangeSource::Binance,
        &test_dates,
    )
    .context("failed to load test-set data for sweep")?;

    // Pre-load all ticks — the sweep reuses them for each parameter combo.
    let ticks: Vec<_> = replay.collect();

    info!(ticks = ticks.len(), "ticks loaded for sweep");

    // Conservative price provider — asks are fixed at market mid values.
    // Replace with a ContractPriceModelProvider for production sweeps.
    #[expect(
        clippy::expect_used,
        reason = "literal values 0.55 and 0.48 are always valid ContractPrice inputs"
    )]
    let price_provider = FixedPriceProvider {
        ask_up: ContractPrice::new(0.55).expect("0.55 is a valid contract price"),
        ask_down: ContractPrice::new(0.48).expect("0.48 is a valid contract price"),
    };

    let bt_config = BacktestConfig {
        initial_balance: cfg.backtest.initial_balance,
        slippage_bps: cfg.backtest.slippage_bps,
        max_position_usdc: cfg.bot.max_position_usdc,
        max_positions_per_window: 1,
    };

    // ── Parameter grid ────────────────────────────────────────────────────────

    let sweep_config = SweepConfig {
        // EarlyDirectional: how many seconds after window open to still enter.
        early_max_times: vec![60, 120, 300],
        // EarlyDirectional: minimum spot magnitude to consider "significant".
        early_min_magnitudes: vec![0.001, 0.003, 0.005],
        // EarlyDirectional: maximum contract ask to enter at.
        early_max_prices: vec![0.55, 0.60, 0.65],
        // MomentumConfirmation: earliest entry time (seconds after open).
        momentum_min_times: vec![120, 300, 600],
        // MomentumConfirmation: latest entry time (seconds after open).
        momentum_max_times: vec![600, 900, 1_800],
        // MomentumConfirmation: minimum sustained magnitude.
        momentum_min_mags: vec![0.003, 0.005, 0.008],
        // MomentumConfirmation: maximum contract ask to enter at.
        momentum_max_prices: vec![0.60, 0.65, 0.70],
    };

    info!(
        early_max_times = sweep_config.early_max_times.len(),
        early_mags = sweep_config.early_min_magnitudes.len(),
        early_prices = sweep_config.early_max_prices.len(),
        mom_min_times = sweep_config.momentum_min_times.len(),
        mom_max_times = sweep_config.momentum_max_times.len(),
        mom_mags = sweep_config.momentum_min_mags.len(),
        mom_prices = sweep_config.momentum_max_prices.len(),
        "running parameter sweep"
    );

    let results = run_sweep(
        &ticks,
        &price_provider,
        &bt_config,
        &sweep_config,
        &enabled_assets,
        &enabled_timeframes,
    );

    info!(combinations_evaluated = results.len(), "sweep complete");

    // ── Print top 10 ─────────────────────────────────────────────────────────

    println!();
    println!("Top 10 parameter combinations (by total P&L):");
    println!();

    let top = results.iter().take(10).enumerate();
    for (i, r) in top {
        println!(
            "  #{rank}: Early({et}s, {em:.3}, {ep:.2}) + Mom({mt}s, {mx}s, {mm:.3}, {mp:.2})",
            rank = i + 1,
            et = r.early_max_time,
            em = r.early_min_mag,
            ep = r.early_max_price,
            mt = r.momentum_min_time,
            mx = r.momentum_max_time,
            mm = r.momentum_min_mag,
            mp = r.momentum_max_price,
        );
        println!(
            "      PnL: {:+.2}  WR: {:.1}%  Trades: {}  Sharpe: {:.2}  PF: {:.2}",
            r.total_pnl,
            r.win_rate * 100.0,
            r.total_trades,
            r.sharpe,
            r.profit_factor,
        );
    }

    if results.is_empty() {
        println!("  (no results — check that test-set data is available)");
    }

    println!();
    Ok(())
}
