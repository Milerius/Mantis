//! Download subcommand: fetch historical candles from Binance for all enabled assets.

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_oracle::downloader::{date_range, download_binance_day};
use pm_types::{Asset, config::BotConfig};
use reqwest::Client;
use tracing::{info, warn};

/// Download one full day of 1-second klines for every enabled asset.
///
/// Iterates the date range defined in `cfg.backtest` and downloads each
/// `(asset, date)` pair.  Already-cached files are skipped automatically by
/// [`download_binance_day`].
///
/// # Errors
///
/// Returns an error if the date range is invalid or any download fails.
pub async fn run_download(cfg: &BotConfig) -> Result<()> {
    let cache_dir = Path::new(&cfg.data.cache_dir);
    let enabled_assets: Vec<Asset> = cfg
        .bot
        .assets
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.asset)
        .collect();

    if enabled_assets.is_empty() {
        warn!("no enabled assets in config — nothing to download");
        return Ok(());
    }

    let dates = date_range(&cfg.backtest.start_date, &cfg.backtest.end_date)
        .context("invalid date range in config")?;

    info!(
        assets = ?enabled_assets,
        start = %cfg.backtest.start_date,
        end   = %cfg.backtest.end_date,
        days  = dates.len(),
        "starting download"
    );

    let client = Client::new();

    for asset in &enabled_assets {
        for date in &dates {
            let count = download_binance_day(&client, *asset, date, cache_dir)
                .await
                .with_context(|| format!("download failed for {asset} on {date}"))?;
            info!(asset = %asset, date = %date, candles = count, "cached");
        }
    }

    info!("download complete");
    Ok(())
}
