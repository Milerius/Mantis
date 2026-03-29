//! Polymarket historical trade data downloader.
//!
//! Downloads per-window trade history from the Polymarket Data API
//! (`data-api.polymarket.com/trades`) for crypto Up/Down markets and caches the
//! results as gzipped JSONL files under `data/polymarket/`.
//!
//! ## Slug format
//!
//! Epoch-based slugs are used for 5m, 15m, and 4h windows:
//! `{asset}-updown-{tf}-{epoch_secs}`
//!
//! where `asset` is one of `btc`, `eth`, `sol`, `xrp` and `epoch_secs` is the
//! window open time as a Unix timestamp in seconds.
//!
//! ## Rate limiting
//!
//! The Polymarket Data API allows 200 requests per 10 seconds.  A conservative
//! 60 ms sleep between requests keeps throughput well within that ceiling.

use std::{
    fs,
    io::{self, BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
    time::Duration,
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use pm_types::{Asset, Timeframe};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::downloader::DownloadError;

// ─── Types ───────────────────────────────────────────────────────────────────

/// A single trade from the Polymarket Data API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolymarketTrade {
    /// Trade direction: `"BUY"` or `"SELL"`.
    pub side: String,
    /// Outcome token traded: `"Up"` or `"Down"`.
    pub outcome: String,
    /// Implied probability / price in the range `[0.0, 1.0]`.
    pub price: f64,
    /// Number of shares traded.
    pub size: f64,
    /// Unix timestamp in seconds.
    pub timestamp: u64,
    /// Market slug (e.g. `btc-updown-15m-1774782000`).
    pub slug: String,
}

// ─── Slug helpers ────────────────────────────────────────────────────────────

/// Return the Polymarket asset prefix for a given [`Asset`].
///
/// Matches the prefix used in epoch-based slug strings:
/// `btc`, `eth`, `sol`, `xrp`.
#[must_use]
pub fn asset_prefix(asset: Asset) -> &'static str {
    match asset {
        Asset::Btc => "btc",
        Asset::Eth => "eth",
        Asset::Sol => "sol",
        Asset::Xrp => "xrp",
    }
}

/// Return the timeframe label used inside Polymarket epoch-based slugs.
///
/// `5m`, `15m`, `1h`, `4h`.
#[must_use]
pub fn timeframe_label(tf: Timeframe) -> &'static str {
    match tf {
        Timeframe::Min5 => "5m",
        Timeframe::Min15 => "15m",
        Timeframe::Hour1 => "1h",
        Timeframe::Hour4 => "4h",
    }
}

/// Build a single Polymarket market slug from the asset, timeframe, and window
/// open time.
///
/// Format: `{asset}-updown-{tf}-{epoch_secs}`
#[must_use]
pub fn market_slug(asset: Asset, tf: Timeframe, epoch_secs: u64) -> String {
    format!(
        "{}-updown-{}-{}",
        asset_prefix(asset),
        timeframe_label(tf),
        epoch_secs
    )
}

/// Generate all `(slug, epoch_secs)` pairs covering every window that opens on
/// the given UTC dates.
///
/// For each date string (`"YYYY-MM-DD"`) the function enumerates every aligned
/// window starting at midnight UTC and advancing by `timeframe.duration_secs()`
/// until the next midnight.  A full day always contains
/// `86400 / timeframe.duration_secs()` windows.
///
/// # Errors
///
/// Returns [`DownloadError::InvalidDate`] if any date string cannot be parsed.
pub fn market_slugs(
    asset: Asset,
    timeframe: Timeframe,
    dates: &[String],
) -> Result<Vec<(String, u64)>, DownloadError> {
    use crate::downloader::parse_date_to_millis;

    let window_secs = timeframe.duration_secs();
    let windows_per_day = 86_400u64 / window_secs;
    let capacity = dates
        .len()
        .saturating_mul(usize::try_from(windows_per_day).unwrap_or(usize::MAX));
    let mut out = Vec::with_capacity(capacity);

    for date in dates {
        // `parse_date_to_millis` returns midnight UTC in milliseconds.
        let midnight_ms = parse_date_to_millis(date)?;
        let midnight_secs = midnight_ms / 1_000;

        for i in 0..windows_per_day {
            let epoch_secs = midnight_secs + i * window_secs;
            let slug = market_slug(asset, timeframe, epoch_secs);
            out.push((slug, epoch_secs));
        }
    }

    Ok(out)
}

// ─── Cache path helpers ──────────────────────────────────────────────────────

/// Build the cache file path for a given slug.
///
/// Files live at `{cache_dir}/{slug}.jsonl.gz`.
#[must_use]
pub fn pm_cache_path(cache_dir: &Path, slug: &str) -> PathBuf {
    cache_dir.join(format!("{slug}.jsonl.gz"))
}

/// Returns `true` if the cache file for `slug` already exists on disk.
#[must_use]
pub fn is_pm_cached(cache_dir: &Path, slug: &str) -> bool {
    pm_cache_path(cache_dir, slug).exists()
}

// ─── Compressed JSONL I/O ────────────────────────────────────────────────────

/// Write a slice of [`PolymarketTrade`]s to a gzipped JSONL file.
///
/// Parent directories are created automatically.
///
/// # Errors
///
/// Returns an [`io::Error`] on any filesystem or serialisation failure.
pub fn write_polymarket_trades(path: &Path, trades: &[PolymarketTrade]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    let mut gz = GzEncoder::new(file, Compression::default());
    for trade in trades {
        let line = serde_json::to_string(trade)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        gz.write_all(line.as_bytes())?;
        gz.write_all(b"\n")?;
    }
    gz.finish()?;
    Ok(())
}

/// Read all [`PolymarketTrade`]s from a gzipped JSONL file.
///
/// # Errors
///
/// Returns an [`io::Error`] if the file cannot be opened, decompressed, or if
/// any line fails to deserialise.
pub fn read_polymarket_trades(cache_dir: &Path, slug: &str) -> io::Result<Vec<PolymarketTrade>> {
    let path = pm_cache_path(cache_dir, slug);
    let file = fs::File::open(path)?;
    let gz = GzDecoder::new(file);
    let reader = BufReader::new(gz);
    let mut trades = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let trade: PolymarketTrade = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        trades.push(trade);
    }
    Ok(trades)
}

// ─── API download ────────────────────────────────────────────────────────────

/// Page size used for Polymarket Data API pagination.
const PM_PAGE_SIZE: usize = 100;

/// Download all trades for a single market window identified by `slug`.
///
/// If the cache file already exists the function reads and returns the cached
/// data without hitting the network.  Otherwise it fetches from the Polymarket
/// Data API, paginates through all results, writes the compressed cache, and
/// returns the trades.
///
/// A 60 ms sleep is inserted **before** each HTTP request to respect the
/// 200 req / 10 s rate limit.
///
/// # Errors
///
/// Returns a [`DownloadError`] on network, API, or I/O failures.
pub async fn download_polymarket_window(
    client: &Client,
    slug: &str,
    cache_dir: &Path,
) -> Result<Vec<PolymarketTrade>, DownloadError> {
    // Fast path: already cached.
    if is_pm_cached(cache_dir, slug) {
        let trades = read_polymarket_trades(cache_dir, slug)?;
        return Ok(trades);
    }

    let mut all_trades: Vec<PolymarketTrade> = Vec::new();
    let mut offset = 0usize;

    loop {
        // Respect rate limit before every request.
        sleep(Duration::from_millis(60)).await;

        let url = format!(
            "https://data-api.polymarket.com/trades?market={slug}&limit={PM_PAGE_SIZE}&offset={offset}"
        );

        let response = client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable body>"));
            return Err(DownloadError::Api(format!("HTTP {status}: {body}")));
        }

        // The API returns a JSON array of trade objects.
        let raw: Vec<serde_json::Value> = response.json().await.map_err(DownloadError::Json)?;
        let batch_len = raw.len();

        for obj in raw {
            let trade = parse_trade_object(&obj, slug)?;
            all_trades.push(trade);
        }

        if batch_len < PM_PAGE_SIZE {
            // Last page reached.
            break;
        }

        offset += batch_len;
    }

    // Persist to cache.
    let path = pm_cache_path(cache_dir, slug);
    write_polymarket_trades(&path, &all_trades)?;

    Ok(all_trades)
}

/// Download all windows for a given `(asset, timeframe, date)` combination.
///
/// Windows that are already cached on disk are skipped (the cache check is a
/// fast `Path::exists` call — no decompression).
///
/// Returns the total number of trades downloaded (excluding cached windows).
///
/// # Errors
///
/// Returns a [`DownloadError`] on network, API, or I/O failures, or if `date`
/// cannot be parsed.
pub async fn download_polymarket_day(
    client: &Client,
    asset: Asset,
    timeframe: Timeframe,
    date: &str,
    cache_dir: &Path,
) -> Result<usize, DownloadError> {
    let dates = vec![date.to_string()];
    let slugs = market_slugs(asset, timeframe, &dates)?;

    let mut total = 0usize;
    for (slug, _epoch) in &slugs {
        let trades = download_polymarket_window(client, slug, cache_dir).await?;
        total += trades.len();
    }

    Ok(total)
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Parse a single JSON object from the API response into a [`PolymarketTrade`].
fn parse_trade_object(
    obj: &serde_json::Value,
    slug: &str,
) -> Result<PolymarketTrade, DownloadError> {
    let side = obj
        .get("side")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DownloadError::Api("missing field `side`".into()))?
        .to_string();

    let outcome = obj
        .get("outcome")
        .and_then(|v| v.as_str())
        .ok_or_else(|| DownloadError::Api("missing field `outcome`".into()))?
        .to_string();

    let price = obj
        .get("price")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| DownloadError::Api("missing or invalid field `price`".into()))?;

    let size = obj
        .get("size")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| DownloadError::Api("missing or invalid field `size`".into()))?;

    let timestamp = obj
        .get("timestamp")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| DownloadError::Api("missing or invalid field `timestamp`".into()))?;

    // The API may echo the slug; if absent we use the one we requested.
    let slug_field = obj
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or(slug)
        .to_string();

    Ok(PolymarketTrade {
        side,
        outcome,
        price,
        size,
        timestamp,
        slug: slug_field,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── slug generation ───────────────────────────────────────────────────────

    #[test]
    fn market_slug_btc_15m_known_epoch() {
        // Epoch 1774782000 is documented in the task spec as a real slug.
        let slug = market_slug(Asset::Btc, Timeframe::Min15, 1_774_782_000);
        assert_eq!(slug, "btc-updown-15m-1774782000");
    }

    #[test]
    fn market_slug_btc_4h_known_epoch() {
        let slug = market_slug(Asset::Btc, Timeframe::Hour4, 1_774_771_200);
        assert_eq!(slug, "btc-updown-4h-1774771200");
    }

    #[test]
    fn market_slug_eth_5m() {
        let slug = market_slug(Asset::Eth, Timeframe::Min5, 1_000_000);
        assert_eq!(slug, "eth-updown-5m-1000000");
    }

    #[test]
    fn market_slug_sol_1h() {
        let slug = market_slug(Asset::Sol, Timeframe::Hour1, 0);
        assert_eq!(slug, "sol-updown-1h-0");
    }

    #[test]
    fn market_slug_xrp_15m() {
        let slug = market_slug(Asset::Xrp, Timeframe::Min15, 900);
        assert_eq!(slug, "xrp-updown-15m-900");
    }

    // ── market_slugs count and alignment ─────────────────────────────────────

    #[test]
    fn market_slugs_15m_single_day_count() {
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min15, &dates).expect("ok");
        // 86400 / 900 = 96 windows per day
        assert_eq!(slugs.len(), 96);
    }

    #[test]
    fn market_slugs_5m_single_day_count() {
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min5, &dates).expect("ok");
        // 86400 / 300 = 288 windows per day
        assert_eq!(slugs.len(), 288);
    }

    #[test]
    fn market_slugs_4h_single_day_count() {
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Hour4, &dates).expect("ok");
        // 86400 / 14400 = 6 windows per day
        assert_eq!(slugs.len(), 6);
    }

    #[test]
    fn market_slugs_1h_single_day_count() {
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Hour1, &dates).expect("ok");
        // 86400 / 3600 = 24 windows per day
        assert_eq!(slugs.len(), 24);
    }

    #[test]
    fn market_slugs_two_days() {
        let dates = vec!["2025-01-01".to_string(), "2025-01-02".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min15, &dates).expect("ok");
        assert_eq!(slugs.len(), 192);
    }

    #[test]
    fn market_slugs_first_epoch_is_midnight_utc() {
        // 2025-01-01 00:00:00 UTC = 1735689600 seconds
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min15, &dates).expect("ok");
        let (_, first_epoch) = &slugs[0];
        assert_eq!(*first_epoch, 1_735_689_600);
    }

    #[test]
    fn market_slugs_second_window_offset() {
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min15, &dates).expect("ok");
        let (_, epoch0) = slugs[0];
        let (_, epoch1) = slugs[1];
        assert_eq!(epoch1 - epoch0, 900);
    }

    #[test]
    fn market_slugs_slug_matches_epoch() {
        let dates = vec!["2025-01-01".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min15, &dates).expect("ok");
        for (slug, epoch) in &slugs {
            let expected = market_slug(Asset::Btc, Timeframe::Min15, *epoch);
            assert_eq!(slug, &expected);
        }
    }

    #[test]
    fn market_slugs_invalid_date() {
        let dates = vec!["not-a-date".to_string()];
        assert!(market_slugs(Asset::Btc, Timeframe::Min15, &dates).is_err());
    }

    // ── known date slug format ────────────────────────────────────────────────

    #[test]
    fn slug_format_march_29_2026() {
        // 2026-03-29 00:00:00 UTC = 1774742400 seconds
        let dates = vec!["2026-03-29".to_string()];
        let slugs = market_slugs(Asset::Btc, Timeframe::Min15, &dates).expect("ok");
        let (first_slug, first_epoch) = &slugs[0];
        // The first window opens at midnight UTC.
        assert_eq!(*first_epoch, 1_774_742_400);
        assert_eq!(first_slug, "btc-updown-15m-1774742400");
    }

    // ── cache path helpers ────────────────────────────────────────────────────

    #[test]
    fn pm_cache_path_format() {
        let dir = Path::new("/tmp/polymarket");
        let path = pm_cache_path(dir, "btc-updown-15m-1774782000");
        assert_eq!(
            path,
            PathBuf::from("/tmp/polymarket/btc-updown-15m-1774782000.jsonl.gz")
        );
    }

    #[test]
    fn is_pm_cached_false_when_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(!is_pm_cached(dir.path(), "btc-updown-15m-1774782000"));
    }

    #[test]
    fn is_pm_cached_true_after_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let slug = "btc-updown-15m-1774782000";
        let path = pm_cache_path(dir.path(), slug);
        write_polymarket_trades(&path, &[]).expect("write");
        assert!(is_pm_cached(dir.path(), slug));
    }

    // ── roundtrip write / read ────────────────────────────────────────────────

    fn sample_trade(ts: u64) -> PolymarketTrade {
        PolymarketTrade {
            side: "BUY".to_string(),
            outcome: "Up".to_string(),
            price: 0.62,
            size: 50.0,
            timestamp: ts,
            slug: "btc-updown-15m-1774782000".to_string(),
        }
    }

    #[test]
    fn roundtrip_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let slug = "btc-updown-15m-0";
        let path = pm_cache_path(dir.path(), slug);
        write_polymarket_trades(&path, &[]).expect("write");
        let result = read_polymarket_trades(dir.path(), slug).expect("read");
        assert!(result.is_empty());
    }

    #[test]
    fn roundtrip_multiple_trades() {
        let dir = tempfile::tempdir().expect("temp dir");
        let slug = "btc-updown-15m-1774782000";
        let trades = vec![
            sample_trade(1_774_782_000),
            sample_trade(1_774_782_300),
            sample_trade(1_774_782_600),
        ];
        let path = pm_cache_path(dir.path(), slug);
        write_polymarket_trades(&path, &trades).expect("write");
        let result = read_polymarket_trades(dir.path(), slug).expect("read");
        assert_eq!(result, trades);
    }

    #[test]
    fn roundtrip_preserves_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let slug = "eth-updown-5m-9000";
        let trade = PolymarketTrade {
            side: "SELL".to_string(),
            outcome: "Down".to_string(),
            price: 0.38,
            size: 100.5,
            timestamp: 9_000,
            slug: slug.to_string(),
        };
        let path = pm_cache_path(dir.path(), slug);
        write_polymarket_trades(&path, &[trade.clone()]).expect("write");
        let result = read_polymarket_trades(dir.path(), slug).expect("read");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], trade);
    }

    #[test]
    fn write_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("a").join("b").join("c");
        let slug = "btc-updown-15m-1";
        let path = pm_cache_path(&nested, slug);
        write_polymarket_trades(&path, &[sample_trade(1)]).expect("write");
        assert!(path.exists());
    }

    // ── asset_prefix / timeframe_label ────────────────────────────────────────

    #[test]
    fn asset_prefix_all_variants() {
        assert_eq!(asset_prefix(Asset::Btc), "btc");
        assert_eq!(asset_prefix(Asset::Eth), "eth");
        assert_eq!(asset_prefix(Asset::Sol), "sol");
        assert_eq!(asset_prefix(Asset::Xrp), "xrp");
    }

    #[test]
    fn timeframe_label_all_variants() {
        assert_eq!(timeframe_label(Timeframe::Min5), "5m");
        assert_eq!(timeframe_label(Timeframe::Min15), "15m");
        assert_eq!(timeframe_label(Timeframe::Hour1), "1h");
        assert_eq!(timeframe_label(Timeframe::Hour4), "4h");
    }
}
