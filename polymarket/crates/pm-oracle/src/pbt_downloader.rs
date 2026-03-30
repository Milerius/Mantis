//! PolyBackTest data downloader and cache manager.
//!
//! Downloads historical market snapshots from the PolyBackTest API and caches
//! them locally as compressed JSONL files. Each market's snapshots are stored in
//! `{cache_dir}/{coin}_{market_type}_{market_id}.jsonl.gz`.
//!
//! Downloads run in parallel (up to 5 concurrent tasks) with exponential-backoff
//! retry on HTTP 429 rate-limit responses. Markets are processed oldest-first.

use std::{
    fs,
    io::{self, BufRead as _, BufReader, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::downloader::DownloadError;
use crate::polybacktest::{PbtClient, PbtMarket, PbtSnapshot};

// ─── Cache path helpers ─────────────────────────────────────────────────────

/// Build the cache file path for a market's snapshots.
#[must_use]
pub fn pbt_cache_path(cache_dir: &Path, coin: &str, market_type: &str, market_id: &str) -> PathBuf {
    cache_dir.join(format!("{coin}_{market_type}_{market_id}.jsonl.gz"))
}

/// Check whether a market's snapshots are already cached.
#[must_use]
pub fn is_pbt_cached(cache_dir: &Path, coin: &str, market_type: &str, market_id: &str) -> bool {
    pbt_cache_path(cache_dir, coin, market_type, market_id).exists()
}

// ─── Read / write cache ─────────────────────────────────────────────────────

/// Write snapshots to a compressed JSONL cache file.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be created or written.
pub fn write_pbt_snapshots(
    path: &Path,
    market: &PbtMarket,
    snapshots: &[PbtSnapshot],
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    let mut gz = GzEncoder::new(file, Compression::fast());

    // First line: market metadata.
    let market_json = serde_json::to_string(market)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    gz.write_all(market_json.as_bytes())?;
    gz.write_all(b"\n")?;

    // Subsequent lines: snapshots.
    for snap in snapshots {
        let line = serde_json::to_string(snap)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        gz.write_all(line.as_bytes())?;
        gz.write_all(b"\n")?;
    }
    gz.finish()?;
    Ok(())
}

/// Read a cached market + snapshots file.
///
/// Returns `(market, snapshots)`.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be read or parsed.
pub fn read_pbt_cache(path: &Path) -> io::Result<(PbtMarket, Vec<PbtSnapshot>)> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(GzDecoder::new(file));
    let mut lines = reader.lines();

    // First line is market metadata.
    let market_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "empty PBT cache file"))??;
    let market: PbtMarket = serde_json::from_str(&market_line)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Remaining lines are snapshots.
    let mut snapshots = Vec::with_capacity(8000);
    for line_result in lines {
        let line = line_result?;
        if line.is_empty() {
            continue;
        }
        let snap: PbtSnapshot = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        snapshots.push(snap);
    }

    Ok((market, snapshots))
}

// ─── Download ───────────────────────────────────────────────────────────────

/// Maximum number of concurrent snapshot download tasks.
const MAX_CONCURRENT: usize = 10;

/// Number of retry attempts on HTTP 429.
const MAX_RETRIES: u32 = 3;

/// Download all resolved markets and their snapshots for a coin and market type.
///
/// Markets are processed oldest-first (the API returns newest first, so the list
/// is reversed before scheduling). Downloads run in parallel with up to
/// [`MAX_CONCURRENT`] concurrent workers. HTTP 429 responses are retried up to
/// [`MAX_RETRIES`] times with increasing backoff (2 s, 4 s, 6 s).
///
/// Caches each market's data to `{cache_dir}/{coin}_{market_type}_{market_id}.jsonl.gz`.
/// Skips markets that are already cached or have no winner (unresolved).
///
/// Returns the number of newly downloaded markets.
///
/// # Errors
///
/// Returns [`DownloadError`] on API or I/O failures. Individual market failures
/// are logged as warnings and skipped.
pub async fn download_pbt_data(
    client: &PbtClient,
    coin: &str,
    market_type: &str,
    cache_dir: &Path,
    max_markets: u32,
) -> Result<usize, DownloadError> {
    // Ensure cache directory exists.
    fs::create_dir_all(cache_dir)?;

    // List all markets (paginated).
    info!(coin, market_type, "listing PBT markets");
    let mut markets = client.list_all_markets(coin, market_type).await?;
    info!(total = markets.len(), coin, market_type, "found PBT markets");

    // API returns newest first — reverse to process oldest first.
    markets.reverse();

    let limit = if max_markets == 0 {
        markets.len()
    } else {
        (max_markets as usize).min(markets.len())
    };

    // Filter: keep only resolved, uncached markets.
    let work: Vec<PbtMarket> = markets
        .into_iter()
        .take(limit)
        .filter(|m| {
            if m.winner.is_none() {
                debug!(market_id = %m.market_id, "skipping unresolved market");
                return false;
            }
            if is_pbt_cached(cache_dir, coin, market_type, &m.market_id) {
                return false;
            }
            true
        })
        .collect();

    let total_work = work.len();
    info!(
        eligible = total_work,
        coin,
        market_type,
        "starting parallel PBT download"
    );

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let downloaded = Arc::new(AtomicUsize::new(0));
    let skipped_err = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::with_capacity(total_work);

    for market in work {
        let permit = semaphore.clone().acquire_owned().await.expect("semaphore closed");

        let market_id = market.market_id.clone();
        let coin_owned = coin.to_string();
        let market_type_owned = market_type.to_string();
        let cache_dir_owned = cache_dir.to_path_buf();
        let downloaded_ctr = downloaded.clone();
        let skipped_ctr = skipped_err.clone();

        // Clone client: reqwest::Client is internally Arc'd so this is cheap.
        let task_client = client.clone_with_same_pool();

        let handle = tokio::spawn(async move {
            let _permit = permit; // released when task completes

            let path = pbt_cache_path(&cache_dir_owned, &coin_owned, &market_type_owned, &market_id);

            for attempt in 0..MAX_RETRIES {
                match task_client.get_all_snapshots(&market_id, &coin_owned).await {
                    Ok(snapshots) => {
                        match write_pbt_snapshots(&path, &market, &snapshots) {
                            Ok(()) => {
                                let n = downloaded_ctr.fetch_add(1, Ordering::Relaxed) + 1;
                                if n % 50 == 0 {
                                    info!(
                                        downloaded = n,
                                        market_id = %market_id,
                                        "PBT download progress"
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(
                                    market_id = %market_id,
                                    error = %e,
                                    "failed to write PBT cache"
                                );
                                skipped_ctr.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        return;
                    }
                    Err(e) if e.to_string().contains("429") => {
                        let wait = Duration::from_secs(u64::from(2 * (attempt + 1)));
                        warn!(
                            market_id = %market_id,
                            attempt,
                            wait_secs = wait.as_secs(),
                            "PBT 429 rate-limited, retrying"
                        );
                        tokio::time::sleep(wait).await;
                    }
                    Err(e) => {
                        warn!(
                            market_id = %market_id,
                            error = %e,
                            "failed to download PBT snapshots"
                        );
                        skipped_ctr.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                }
            }

            // All retries exhausted.
            warn!(market_id = %market_id, "exhausted retries for PBT market, skipping");
            skipped_ctr.fetch_add(1, Ordering::Relaxed);
        });

        handles.push(handle);
    }

    // Await all tasks.
    for handle in handles {
        if let Err(e) = handle.await {
            warn!(error = ?e, "PBT download task panicked");
        }
    }

    let n_downloaded = downloaded.load(Ordering::Relaxed);
    let n_errors = skipped_err.load(Ordering::Relaxed);

    info!(
        downloaded = n_downloaded,
        errors = n_errors,
        coin,
        market_type,
        "PBT download complete"
    );
    Ok(n_downloaded)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_market() -> PbtMarket {
        PbtMarket {
            market_id: "test_market_001".into(),
            slug: "btc-15m-2026-01-01".into(),
            market_type: "15m".into(),
            start_time: "2026-01-01T00:00:00Z".into(),
            end_time: "2026-01-01T00:15:00Z".into(),
            btc_price_start: Some(95000.0),
            btc_price_end: Some(95150.0),
            winner: Some("Up".into()),
            clob_token_up: None,
            clob_token_down: None,
        }
    }

    fn sample_snapshots() -> Vec<PbtSnapshot> {
        vec![
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:00:00Z".into(),
                market_id: None,
                btc_price: Some(95000.0),
                price_up: Some(0.50),
                price_down: Some(0.51),
                orderbook_up: None,
                orderbook_down: None,
            },
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:07:30Z".into(),
                market_id: None,
                btc_price: Some(95100.0),
                price_up: Some(0.55),
                price_down: Some(0.46),
                orderbook_up: None,
                orderbook_down: None,
            },
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:14:59Z".into(),
                market_id: None,
                btc_price: Some(95150.0),
                price_up: Some(0.62),
                price_down: Some(0.39),
                orderbook_up: None,
                orderbook_down: None,
            },
        ]
    }

    #[test]
    fn cache_path_format() {
        let dir = Path::new("/tmp/pbt_cache");
        let path = pbt_cache_path(dir, "btc", "15m", "abc123");
        assert_eq!(
            path.to_str().expect("valid path"),
            "/tmp/pbt_cache/btc_15m_abc123.jsonl.gz"
        );
    }

    #[test]
    fn roundtrip_write_read_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        let market = sample_market();
        let snapshots = sample_snapshots();

        let path = pbt_cache_path(dir.path(), "btc", "15m", &market.market_id);
        write_pbt_snapshots(&path, &market, &snapshots).expect("write should succeed");

        assert!(path.exists(), "cache file should exist");

        let (read_market, read_snaps) = read_pbt_cache(&path).expect("read should succeed");
        assert_eq!(read_market.market_id, market.market_id);
        assert_eq!(read_market.winner, market.winner);
        assert_eq!(read_snaps.len(), 3);
        assert!((read_snaps[0].btc_price.expect("btc_price") - 95000.0).abs() < 1e-6);
        assert!((read_snaps[2].price_up.expect("price_up") - 0.62).abs() < 1e-6);
    }

    #[test]
    fn is_cached_returns_false_for_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(!is_pbt_cached(dir.path(), "btc", "15m", "nonexistent"));
    }

    #[test]
    fn is_cached_returns_true_after_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let market = sample_market();
        let path = pbt_cache_path(dir.path(), "btc", "15m", &market.market_id);
        write_pbt_snapshots(&path, &market, &[]).expect("write");
        assert!(is_pbt_cached(dir.path(), "btc", "15m", &market.market_id));
    }
}
