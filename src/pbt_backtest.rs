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
use pm_executor::{ModelPriceProvider, run_backtest_v2};
use pm_oracle::contract_model;
use pm_oracle::pbt_replay::{PbtObservation, pbt_to_price_observations};
use pm_oracle::PbtReplay;
use pm_signal::build_instances_from_config;
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

    // Build independent strategy instances from TOML config — parameters are
    // fully configurable via [[bot.strategies]] blocks.  Each instance owns its
    // balance, positions, and risk parameters.
    let mut instances: Vec<Box<dyn pm_types::StrategyInstance>> =
        build_instances_from_config(&cfg.bot.strategies)
            .into_iter()
            .map(|i| Box::new(i) as Box<dyn pm_types::StrategyInstance>)
            .collect();

    info!(count = instances.len(), "strategy instances created for PBT backtest");

    // Use the PBT-calibrated model as the price provider.
    let price_provider = ModelPriceProvider {
        model: contract_model,
        half_spread: 0.005, // tighter spread for PBT data — it's from real orderbooks
    };

    let result_v2 = run_backtest_v2(
        ticks.into_iter(),
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
        "PBT backtest complete (v2)"
    );

    // Export results.
    let pbt_log = log_dir.join("pbt");
    std::fs::create_dir_all(&pbt_log)
        .context("failed to create PBT log directory")?;

    // Compute a combined summary for export.
    let combined_summary = pm_bookkeeper::compute_summary(&all_trades);

    export_trades_csv(&pbt_log.join("trades.csv"), &all_trades)
        .context("failed to export PBT trades.csv")?;
    export_equity_curve(&pbt_log.join("equity.csv"), &all_trades)
        .context("failed to export PBT equity.csv")?;
    export_summary(&pbt_log.join("summary.json"), &combined_summary)
        .context("failed to export PBT summary.json")?;

    info!(log_dir = %pbt_log.display(), "PBT results exported");
    Ok(())
}
