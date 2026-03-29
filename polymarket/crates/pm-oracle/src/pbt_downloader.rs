//! PolyBackTest data downloader and cache manager.
//!
//! Downloads historical market snapshots from the PolyBackTest API and caches
//! them locally as compressed JSONL files. Each market's snapshots are stored in
//! `{cache_dir}/{coin}_{market_type}_{market_id}.jsonl.gz`.

use std::{
    fs,
    io::{self, BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
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
    let mut snapshots = Vec::new();
    for line_result in lines {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }
        let snap: PbtSnapshot = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        snapshots.push(snap);
    }

    Ok((market, snapshots))
}

// ─── Download ───────────────────────────────────────────────────────────────

/// Download all resolved markets and their snapshots for a coin and market type.
///
/// Caches each market's data to `{cache_dir}/{coin}_{market_type}_{market_id}.jsonl.gz`.
/// Skips markets that are already cached.
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
    let markets = client.list_all_markets(coin, market_type).await?;
    info!(total = markets.len(), coin, market_type, "found PBT markets");

    let limit = if max_markets == 0 {
        markets.len()
    } else {
        (max_markets as usize).min(markets.len())
    };

    let mut downloaded: usize = 0;
    let mut skipped: usize = 0;

    for (i, market) in markets.iter().take(limit).enumerate() {
        // Skip already cached.
        if is_pbt_cached(cache_dir, coin, market_type, &market.market_id) {
            skipped += 1;
            continue;
        }

        // Only download resolved markets (they have a winner).
        if market.winner.is_none() {
            debug!(market_id = %market.market_id, "skipping unresolved market");
            continue;
        }

        // Rate limit: 300 req/min = 5 req/sec. Wait 250ms between markets.
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;

        // Download all snapshots.
        match client.get_all_snapshots(&market.market_id, coin).await {
            Ok(snapshots) => {
                let path = pbt_cache_path(cache_dir, coin, market_type, &market.market_id);
                write_pbt_snapshots(&path, market, &snapshots)?;
                downloaded += 1;

                if downloaded.is_multiple_of(50) {
                    info!(
                        downloaded,
                        skipped,
                        progress = i + 1,
                        total = limit,
                        "PBT download progress"
                    );
                }
            }
            Err(e) => {
                warn!(
                    market_id = %market.market_id,
                    error = %e,
                    "failed to download PBT snapshots"
                );
            }
        }
    }

    info!(
        downloaded,
        skipped,
        coin,
        market_type,
        "PBT download complete"
    );
    Ok(downloaded)
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
            },
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:07:30Z".into(),
                market_id: None,
                btc_price: Some(95100.0),
                price_up: Some(0.55),
                price_down: Some(0.46),
            },
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:14:59Z".into(),
                market_id: None,
                btc_price: Some(95150.0),
                price_up: Some(0.62),
                price_down: Some(0.39),
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
        assert!((read_snaps[0].btc_price - 95000.0).abs() < 1e-6);
        assert!((read_snaps[2].price_up - 0.62).abs() < 1e-6);
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
