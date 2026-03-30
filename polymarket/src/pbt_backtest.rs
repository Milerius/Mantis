//! `pbt-backtest` subcommand: calibrate and backtest using real `PolyBackTest` prices.
//!
//! Unlike the standard `backtest` command that uses model-estimated contract prices,
//! this command uses real historical snapshot prices from the `PolyBackTest` API.
//!
//! The approach:
//! 1. Load cached PBT snapshots via [`PbtReplay`].
//! 2. Convert PBT observations to [`PriceObservation`]s and calibrate a
//!    [`ContractPriceModel`] from them.
//! 3. Extract ticks from the PBT observations.
//! 4. Run the standard 4-strategy backtest using the PBT-calibrated model.

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_bookkeeper::{export_equity_curve, export_summary, export_trades_csv};
use pm_executor::{BacktestConfig, ModelPriceProvider, run_backtest};
use pm_oracle::contract_model;
use pm_oracle::pbt_replay::{PbtObservation, pbt_to_price_observations};
use pm_oracle::PbtReplay;
use pm_signal::build_engine_from_config;
use pm_types::{Asset, Timeframe, config::BotConfig};
use tracing::info;

/// Run the `pbt-backtest` subcommand.
///
/// Loads PBT cached data, calibrates a contract price model from real snapshot
/// prices, and runs the 4-strategy backtest.
///
/// # Errors
///
/// Returns an error if cached data is missing or the backtest fails.
#[expect(clippy::too_many_lines, reason = "backtest driver — splitting would fragment the logical flow")]
pub fn run_pbt_backtest(cfg: &BotConfig) -> Result<()> {
    let cache_dir = Path::new(&cfg.data.cache_dir).join("polybacktest");
    let log_dir = Path::new(&cfg.data.log_dir);

    // Collect enabled assets and timeframes.
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

    // Map Asset -> coin string for PBT.
    let coin_map = |asset: Asset| -> &'static str {
        match asset {
            Asset::Btc => "btc",
            Asset::Eth => "eth",
            Asset::Sol => "sol",
            Asset::Xrp => "xrp",
        }
    };

    // Map Timeframe -> market_type string for PBT.
    let tf_map = |tf: Timeframe| -> &'static str {
        match tf {
            Timeframe::Min5 => "5m",
            Timeframe::Min15 => "15m",
            Timeframe::Hour1 => "1h",
            Timeframe::Hour4 => "4h",
        }
    };

    // Load PBT data for all enabled (asset, timeframe) combinations.
    let mut all_observations: Vec<PbtObservation> = Vec::new();

    for &asset in &enabled_assets {
        let coin = coin_map(asset);
        for &tf in &enabled_timeframes {
            let mt = tf_map(tf);
            info!(coin, market_type = mt, "loading PBT replay data");
            match PbtReplay::load(&cache_dir, coin, mt) {
                Ok(replay) => {
                    info!(
                        observations = replay.len(),
                        coin,
                        market_type = mt,
                        "loaded PBT replay"
                    );
                    all_observations.extend(replay);
                }
                Err(e) => {
                    tracing::warn!(
                        coin,
                        market_type = mt,
                        error = %e,
                        "no PBT cache found, skipping"
                    );
                }
            }
        }
    }

    if all_observations.is_empty() {
        anyhow::bail!(
            "no PBT data found in {}. Run `polymarket pbt-download` first.",
            cache_dir.display()
        );
    }

    info!(
        total_observations = all_observations.len(),
        "building contract price model from PBT data"
    );

    // Build PriceObservations from PBT data and calibrate model.
    let mut price_obs = Vec::new();
    for &asset in &enabled_assets {
        let coin = coin_map(asset);
        for &tf in &enabled_timeframes {
            let asset_tf_obs: Vec<&PbtObservation> = all_observations
                .iter()
                .filter(|o| o.tick.asset == asset)
                .collect();

            let tf_obs: Vec<PbtObservation> = asset_tf_obs
                .into_iter()
                .cloned()
                .collect();

            let converted = pbt_to_price_observations(&tf_obs, asset, tf);
            info!(
                coin,
                market_type = tf_map(tf),
                observations = converted.len(),
                "converted PBT observations"
            );
            price_obs.extend(converted);
        }
    }

    let contract_model = contract_model::calibrate(&price_obs, 3);
    info!("contract price model calibrated from PBT data");

    // Extract ticks from observations for the backtest engine.
    let ticks: Vec<pm_types::Tick> = all_observations
        .iter()
        .map(|o| o.tick)
        .collect();

    info!(ticks = ticks.len(), "feeding ticks to backtest engine");

    // Build strategy engine from TOML config — parameters are fully
    // configurable via [[bot.strategies]] blocks.  Falls back to sensible
    // defaults when the key is absent from the config file.
    let engine = build_engine_from_config(&cfg.bot.strategies);

    // Use the PBT-calibrated model as the price provider.
    let price_provider = ModelPriceProvider {
        model: contract_model,
        half_spread: 0.005, // tighter spread for PBT data — it's from real orderbooks
    };

    let bt_config = BacktestConfig {
        initial_balance: cfg.backtest.initial_balance,
        slippage_bps: cfg.backtest.slippage_bps,
        max_position_usdc: cfg.bot.max_position_usdc,
        max_positions_per_window: 1,
    };

    let result = run_backtest(
        ticks.into_iter(),
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
        "PBT backtest complete"
    );

    // Per-strategy breakdown.
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
    let pbt_log = log_dir.join("pbt");
    std::fs::create_dir_all(&pbt_log)
        .context("failed to create PBT log directory")?;

    export_trades_csv(&pbt_log.join("trades.csv"), &result.trades)
        .context("failed to export PBT trades.csv")?;
    export_equity_curve(&pbt_log.join("equity.csv"), &result.trades)
        .context("failed to export PBT equity.csv")?;
    export_summary(&pbt_log.join("summary.json"), &result.summary)
        .context("failed to export PBT summary.json")?;

    info!(log_dir = %pbt_log.display(), "PBT results exported");
    Ok(())
}
