//! Binance REST API downloader for 1-second historical klines.
//!
//! Downloads a full day of 1-second candle data from Binance and stores it in
//! the compressed JSONL cache maintained by [`crate::storage`]. Each calendar
//! day requires ~87 paginated requests (86 400 seconds ÷ 1 000 per page).

use std::path::Path;

use chrono::NaiveDate;
use pm_types::{Asset, ExchangeSource};
use reqwest::Client;
use thiserror::Error;

use crate::storage::{self, Candle};

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors that can occur while downloading historical candle data.
#[derive(Debug, Error)]
pub enum DownloadError {
    /// An HTTP transport error from `reqwest`.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON deserialisation error.
    #[error("json parse error: {0}")]
    Json(#[source] reqwest::Error),

    /// A filesystem I/O error (e.g. writing the cache file).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The Binance API returned an error payload.
    #[error("binance api error: {0}")]
    Api(String),

    /// A date string could not be parsed.
    #[error("invalid date `{0}`: expected YYYY-MM-DD")]
    InvalidDate(String),
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Download one calendar day of 1-second klines for `asset` from Binance.
///
/// If the cache file for `(asset, Binance, date)` already exists the function
/// returns immediately with the count of candles stored on disk.
///
/// # Parameters
///
/// - `client` — a shared [`reqwest::Client`] for connection reuse.
/// - `asset`  — the underlying asset (symbol derived via
///   [`Asset::binance_symbol`]).
/// - `date`   — calendar date in `"YYYY-MM-DD"` format (UTC).
/// - `cache_dir` — directory where compressed cache files are stored.
///
/// # Returns
///
/// The total number of candles available for the requested day.
///
/// # Errors
///
/// Returns a [`DownloadError`] on network, API, or I/O failures.
pub async fn download_binance_day(
    client: &Client,
    asset: Asset,
    date: &str,
    cache_dir: &Path,
) -> Result<usize, DownloadError> {
    // Fast path: already cached.
    if storage::is_cached(cache_dir, asset, ExchangeSource::Binance, date) {
        let path = storage::cache_path(cache_dir, asset, ExchangeSource::Binance, date);
        let candles = storage::read_candles(&path)?;
        return Ok(candles.len());
    }

    let symbol = asset.binance_symbol();
    let start_ms = parse_date_to_millis(date)?;
    // A UTC day is exactly 86 400 000 ms.
    let end_ms = start_ms + 86_400_000u64;

    let mut all_candles: Vec<Candle> = Vec::with_capacity(86_400);
    let mut cursor_ms = start_ms;

    while cursor_ms < end_ms {
        let url = format!(
            "https://api.binance.com/api/v3/klines?symbol={symbol}&interval=1s&startTime={cursor_ms}&limit=1000"
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

        // Each kline is a JSON array:
        // [openTime, open, high, low, close, volume, closeTime, ...]
        let raw: Vec<serde_json::Value> = response.json().await.map_err(DownloadError::Json)?;

        if raw.is_empty() {
            break;
        }

        let batch_len = raw.len();
        let mut last_close_time: u64 = cursor_ms;

        for row in raw {
            let arr = row
                .as_array()
                .ok_or_else(|| DownloadError::Api("kline row is not an array".into()))?;

            let open_time_ms = parse_u64_value(&arr[0])?;
            let open = parse_f64_str(&arr[1])?;
            let high = parse_f64_str(&arr[2])?;
            let low = parse_f64_str(&arr[3])?;
            let close = parse_f64_str(&arr[4])?;
            let volume = parse_f64_str(&arr[5])?;
            let close_time_ms = parse_u64_value(&arr[6])?;

            last_close_time = close_time_ms;

            // Only keep candles within the requested day.
            if open_time_ms >= end_ms {
                break;
            }

            all_candles.push(Candle {
                open_time_ms,
                close_time_ms,
                open,
                high,
                low,
                close,
                volume,
            });
        }

        // Advance the cursor past the last received close time.
        cursor_ms = last_close_time + 1;

        // Fewer than 1 000 rows means we've reached the end.
        if batch_len < 1_000 {
            break;
        }
    }

    let count = all_candles.len();
    let path = storage::cache_path(cache_dir, asset, ExchangeSource::Binance, date);
    storage::write_candles(&path, &all_candles)?;
    Ok(count)
}

/// Generate an inclusive list of `"YYYY-MM-DD"` date strings from `start` to
/// `end`.
///
/// # Errors
///
/// Returns [`DownloadError::InvalidDate`] if either `start` or `end` cannot be
/// parsed as a `NaiveDate`.
pub fn date_range(start: &str, end: &str) -> Result<Vec<String>, DownloadError> {
    let start_date = parse_naive_date(start)?;
    let end_date = parse_naive_date(end)?;

    let mut dates = Vec::new();
    let mut current = start_date;
    while current <= end_date {
        dates.push(current.to_string());
        current = current.succ_opt().unwrap_or(end_date);
        // Guard against infinite loop if succ fails at boundary.
        if current > end_date {
            break;
        }
    }
    Ok(dates)
}

/// Parse a `"YYYY-MM-DD"` date string to a Unix timestamp in milliseconds
/// (midnight UTC).
///
/// # Errors
///
/// Returns [`DownloadError::InvalidDate`] if the string cannot be parsed.
pub fn parse_date_to_millis(date: &str) -> Result<u64, DownloadError> {
    use chrono::{TimeZone as _, Utc};
    let naive = parse_naive_date(date)?;
    let dt = Utc
        .from_utc_datetime(&naive.and_hms_opt(0, 0, 0).ok_or_else(|| {
            DownloadError::InvalidDate(date.to_string())
        })?)
        .timestamp_millis();
    u64::try_from(dt).map_err(|_| DownloadError::InvalidDate(date.to_string()))
}

// ─── Private helpers ─────────────────────────────────────────────────────────

fn parse_naive_date(date: &str) -> Result<NaiveDate, DownloadError> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| DownloadError::InvalidDate(date.to_string()))
}

/// Parse a `serde_json::Value` that is either a `u64` integer or a string
/// representation of one.
fn parse_u64_value(val: &serde_json::Value) -> Result<u64, DownloadError> {
    if let Some(n) = val.as_u64() {
        return Ok(n);
    }
    if let Some(s) = val.as_str() {
        return s
            .parse::<u64>()
            .map_err(|e| DownloadError::Api(format!("cannot parse u64 `{s}`: {e}")));
    }
    Err(DownloadError::Api(format!(
        "expected integer or string, got: {val}"
    )))
}

/// Parse a price or volume field that Binance returns as a quoted decimal
/// string, e.g. `"42583.12"`.
fn parse_f64_str(val: &serde_json::Value) -> Result<f64, DownloadError> {
    let s = val
        .as_str()
        .ok_or_else(|| DownloadError::Api(format!("expected string price, got: {val}")))?;
    s.parse::<f64>()
        .map_err(|e| DownloadError::Api(format!("cannot parse f64 `{s}`: {e}")))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_range_single_day() {
        let dates = date_range("2025-01-01", "2025-01-01").expect("valid range");
        assert_eq!(dates, vec!["2025-01-01"]);
    }

    #[test]
    fn date_range_three_days() {
        let dates = date_range("2025-01-01", "2025-01-03").expect("valid range");
        assert_eq!(dates, vec!["2025-01-01", "2025-01-02", "2025-01-03"]);
    }

    #[test]
    fn date_range_month_boundary() {
        let dates = date_range("2025-01-30", "2025-02-02").expect("valid range");
        assert_eq!(
            dates,
            vec!["2025-01-30", "2025-01-31", "2025-02-01", "2025-02-02"]
        );
    }

    #[test]
    fn date_range_invalid_start() {
        assert!(date_range("not-a-date", "2025-01-01").is_err());
    }

    #[test]
    fn date_range_invalid_end() {
        assert!(date_range("2025-01-01", "2025-99-99").is_err());
    }

    #[test]
    fn parse_date_to_millis_known_value() {
        // 2025-10-01 00:00:00 UTC = 1759276800000 ms
        let ms = parse_date_to_millis("2025-10-01").expect("valid date");
        assert_eq!(ms, 1_759_276_800_000);
    }

    #[test]
    fn parse_date_to_millis_epoch() {
        let ms = parse_date_to_millis("1970-01-01").expect("epoch");
        assert_eq!(ms, 0);
    }

    #[test]
    fn parse_date_to_millis_invalid() {
        assert!(parse_date_to_millis("2025-13-01").is_err());
        assert!(parse_date_to_millis("hello").is_err());
    }
}
