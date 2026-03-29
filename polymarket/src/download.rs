//! Download subcommand: fetch historical candles from Binance and Polymarket trade data.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context as _, Result};
use pm_oracle::downloader::{date_range, download_binance_day};
use pm_oracle::polymarket::{download_polymarket_window, market_slugs};
use pm_types::{Asset, Timeframe, config::BotConfig};
use reqwest::Client;
use tokio::sync::Semaphore;
use tracing::{info, warn};

/// Download one full day of 1-second klines for every enabled asset from Binance,
/// then download 15m Polymarket trade data for the same date range.
///
/// Iterates the date range defined in `cfg.backtest` and downloads each
/// `(asset, date)` pair.  Already-cached files are skipped automatically by
/// the individual download functions.
///
/// Polymarket downloads are limited to the `Min15` timeframe initially — the
/// 15m slug pattern is the most reliably available in the data API.  A 200 ms
/// sleep between days provides extra headroom beyond the 60 ms per-request
/// rate-limit sleep already built into [`download_polymarket_day`].
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

    // ── Binance 1-second candles ──────────────────────────────────────────────

    for asset in &enabled_assets {
        for date in &dates {
            let count = download_binance_day(&client, *asset, date, cache_dir)
                .await
                .with_context(|| format!("binance download failed for {asset} on {date}"))?;
            info!(asset = %asset, date = %date, candles = count, "binance cached");
        }
    }

    info!("binance download complete");

    // ── Polymarket 15m trade windows (concurrent) ──────────────────────────────

    let pm_cache_dir = cache_dir.join("polymarket");

    // Collect all slugs across all assets and dates.
    let mut all_slugs: Vec<(String, Asset)> = Vec::new();
    for &asset in &enabled_assets {
        let slugs = market_slugs(asset, Timeframe::Min15, &dates)
            .context("invalid polymarket slug generation")?;
        for (slug, _epoch) in slugs {
            all_slugs.push((slug, asset));
        }
    }

    info!(
        total_windows = all_slugs.len(),
        assets = ?enabled_assets,
        "starting polymarket download (concurrent)"
    );

    // 3 concurrent requests — conservative to avoid Cloudflare 429s.
    // Each request has a 60ms built-in delay, so 3 concurrent ≈ 50 req/10s.
    let semaphore = Arc::new(Semaphore::new(3));
    let client = Arc::new(client);
    let pm_cache_dir = Arc::new(pm_cache_dir);
    let downloaded = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let total = all_slugs.len();

    let mut handles = Vec::with_capacity(all_slugs.len());
    for (slug, asset) in all_slugs {
        let sem = Arc::clone(&semaphore);
        let cl = Arc::clone(&client);
        let dir = Arc::clone(&pm_cache_dir);
        let dl = Arc::clone(&downloaded);
        let sk = Arc::clone(&skipped);
        let fl = Arc::clone(&failed);

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await;
            match download_polymarket_window(&cl, &slug, &dir).await {
                Ok(trades) => {
                    if trades.is_empty() {
                        sk.fetch_add(1, Ordering::Relaxed);
                    } else {
                        dl.fetch_add(1, Ordering::Relaxed);
                    }
                    let done = dl.load(Ordering::Relaxed) + sk.load(Ordering::Relaxed) + fl.load(Ordering::Relaxed);
                    if done % 100 == 0 {
                        info!(progress = done, total, asset = %asset, "polymarket download progress");
                    }
                }
                Err(e) => {
                    fl.fetch_add(1, Ordering::Relaxed);
                    warn!(slug = %slug, error = %e, "polymarket window download failed");
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    info!(
        downloaded = downloaded.load(Ordering::Relaxed),
        skipped = skipped.load(Ordering::Relaxed),
        failed = failed.load(Ordering::Relaxed),
        "polymarket download complete"
    );

    Ok(())
}
