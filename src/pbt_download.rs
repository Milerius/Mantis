//! `pbt-download` subcommand: download `PolyBackTest` historical snapshots.

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_oracle::PbtClient;
use pm_oracle::pbt_downloader::download_pbt_data;
use pm_types::config::BotConfig;
use tracing::info;

/// Read the `PolyBackTest` API key from `config/secrets.toml`.
///
/// Expected format:
/// ```toml
/// [polybacktest]
/// api_key = "pdm_..."
/// ```
///
/// # Errors
///
/// Returns an error if the file is missing, unreadable, or lacks the expected key.
fn read_pbt_api_key() -> Result<String> {
    let path = "config/secrets.toml";
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read secrets file `{path}`"))?;

    let table: toml::Table = toml::from_str(&src)
        .with_context(|| format!("cannot parse secrets file `{path}`"))?;

    let key = table
        .get("polybacktest")
        .and_then(|v| v.get("api_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing polybacktest.api_key in `{path}`"))?;

    Ok(key.to_string())
}

/// Run the `pbt-download` subcommand.
///
/// Downloads BTC and ETH 15m markets (the primary backtesting targets) from the
/// `PolyBackTest` API.
///
/// # Errors
///
/// Returns an error if the API key is missing or if downloads fail.
pub async fn run_pbt_download(cfg: &BotConfig) -> Result<()> {
    let api_key = read_pbt_api_key()?;
    let client = PbtClient::new(api_key);

    let cache_dir = Path::new(&cfg.data.cache_dir).join("polybacktest");

    // Coins and market types to download.
    // Prioritize missing/sparse data first, heavy datasets last.
    let coins_types: Vec<(&str, &str)> = vec![
        ("xrp", "15m"),  // 0 files — highest priority
        ("xrp", "5m"),   // 0 files
        ("eth", "15m"),  // 200 files — need more
        ("eth", "5m"),   // 200 files
        ("sol", "15m"),  // 200 files
        ("sol", "5m"),   // 200 files
        ("btc", "5m"),   // 271 files — need more
        ("btc", "1h"),   // 0 files
        ("eth", "1h"),   // 0 files
        ("btc", "4h"),   // 0 files
        ("eth", "4h"),   // 0 files
        ("btc", "15m"),  // 2384 files — mostly cached, last
    ];

    let mut total_downloaded: usize = 0;

    for (coin, market_type) in &coins_types {
        info!(coin, market_type, "downloading PBT data");

        // Cool down between coin/timeframe combos to avoid PBT burst limit.
        if total_downloaded > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }

        match download_pbt_data(
            &client,
            coin,
            market_type,
            &cache_dir,
            0, // no limit — download all
        )
        .await
        {
            Ok(count) => {
                total_downloaded += count;
                info!(coin, market_type, count, "PBT download batch complete");
            }
            Err(e) => {
                tracing::warn!(coin, market_type, error = %e, "PBT download failed — skipping");
            }
        }
    }

    info!(total_downloaded, "PBT download complete");
    Ok(())
}
