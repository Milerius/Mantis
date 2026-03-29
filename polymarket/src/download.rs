//! Download subcommand: fetch historical candles from Binance and Polymarket trade data.
//!
//! Both Binance and Polymarket downloads are parallelized with bounded concurrency
//! to maximize throughput while respecting API rate limits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context as _, Result};
use pm_oracle::downloader::{date_range, download_binance_day};
use pm_oracle::polymarket::{download_polymarket_window, market_slugs};
use pm_types::{Asset, Timeframe, config::BotConfig};
use reqwest::Client;
use tokio::sync::Semaphore;
use tracing::{info, warn};

/// Download historical data from Binance and Polymarket concurrently.
///
/// # Errors
///
/// Returns an error if the date range is invalid. Individual download
/// failures are logged as warnings and skipped.
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
        start  = %cfg.backtest.start_date,
        end    = %cfg.backtest.end_date,
        days   = dates.len(),
        "starting download"
    );

    let client = Arc::new(Client::new());

    // Run Binance and Polymarket downloads concurrently.
    let binance_fut = download_binance_concurrent(
        Arc::clone(&client),
        &enabled_assets,
        &dates,
        cache_dir.to_path_buf(),
    );
    let pm_fut = download_polymarket_concurrent(
        Arc::clone(&client),
        &enabled_assets,
        &dates,
        cache_dir.join("polymarket"),
    );

    let (binance_res, pm_res) = tokio::join!(binance_fut, pm_fut);
    binance_res?;
    pm_res?;

    info!("all downloads complete");
    Ok(())
}

/// Download Binance 1-second candles with bounded concurrency.
///
/// Binance rate limit: 1200 req/min. Each day needs ~87 requests.
/// At 5 concurrent days: ~435 req/batch, well within limits.
async fn download_binance_concurrent(
    client: Arc<Client>,
    assets: &[Asset],
    dates: &[String],
    cache_dir: PathBuf,
) -> Result<()> {
    // Collect all (asset, date) pairs.
    let mut jobs: Vec<(Asset, String)> = Vec::new();
    for &asset in assets {
        for date in dates {
            jobs.push((asset, date.clone()));
        }
    }

    let total = jobs.len();
    let done = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    // 5 concurrent downloads — each day is ~87 sequential API calls internally,
    // so 5 concurrent = ~435 req burst, settling to ~5 req/s sustained.
    // Binance allows 1200 req/min = 20 req/s, so we're well under.
    let semaphore = Arc::new(Semaphore::new(5));

    let mut handles = Vec::with_capacity(jobs.len());
    for (asset, date) in jobs {
        let sem = Arc::clone(&semaphore);
        let cl = Arc::clone(&client);
        let dir = cache_dir.clone();
        let dn = Arc::clone(&done);
        let fl = Arc::clone(&failed);

        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await;
            match download_binance_day(&cl, asset, &date, &dir).await {
                Ok(count) => {
                    let n = dn.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(10) || count > 0 {
                        info!(
                            asset = %asset, date = %date, candles = count,
                            progress = n, total, "binance"
                        );
                    }
                }
                Err(e) => {
                    fl.fetch_add(1, Ordering::Relaxed);
                    warn!(asset = %asset, date = %date, error = %e, "binance download failed");
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    info!(
        done = done.load(Ordering::Relaxed),
        failed = failed.load(Ordering::Relaxed),
        "binance download complete"
    );
    Ok(())
}

/// Download Polymarket 15m trade windows with bounded concurrency.
async fn download_polymarket_concurrent(
    client: Arc<Client>,
    assets: &[Asset],
    dates: &[String],
    pm_cache_dir: PathBuf,
) -> Result<()> {
    // Collect all slugs.
    let mut all_slugs: Vec<(String, Asset)> = Vec::new();
    for &asset in assets {
        let slugs = market_slugs(asset, Timeframe::Min15, dates)
            .context("invalid polymarket slug generation")?;
        for (slug, _epoch) in slugs {
            all_slugs.push((slug, asset));
        }
    }

    let total = all_slugs.len();
    let downloaded = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    info!(total_windows = total, "starting polymarket download");

    // 5 concurrent — Polymarket allows 200 req/10s.
    // Each request has 60ms built-in delay, so 5 concurrent ≈ 83 req/10s.
    let semaphore = Arc::new(Semaphore::new(5));
    let pm_cache_dir = Arc::new(pm_cache_dir);

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
                    let done = dl.load(Ordering::Relaxed)
                        + sk.load(Ordering::Relaxed)
                        + fl.load(Ordering::Relaxed);
                    if done.is_multiple_of(500) {
                        info!(
                            progress = done, total, asset = %asset,
                            "polymarket progress"
                        );
                    }
                }
                Err(e) => {
                    fl.fetch_add(1, Ordering::Relaxed);
                    // Only warn on non-404 errors (404 = window doesn't exist)
                    if !e.to_string().contains("404") {
                        warn!(slug = %slug, error = %e, "polymarket download failed");
                    }
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
