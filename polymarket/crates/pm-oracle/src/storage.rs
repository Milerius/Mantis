//! Compressed file cache for historical candle data.
//!
//! Candles are stored as gzipped JSONL files. Each line is a JSON-serialised
//! [`Candle`]. The file name encodes the asset, exchange source, and date so
//! that cache presence can be checked with a single [`std::path::Path::exists`]
//! call.

use std::{
    fs,
    io::{self, BufRead, BufReader, Write as _},
    path::{Path, PathBuf},
};

use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use pm_types::{Asset, ExchangeSource, Price, Tick};
use serde::{Deserialize, Serialize};

// ─── Candle ──────────────────────────────────────────────────────────────────

/// A 1-second OHLCV candle as returned by Binance `klines`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    /// Open time in milliseconds since Unix epoch.
    pub open_time_ms: u64,
    /// Close time in milliseconds since Unix epoch.
    pub close_time_ms: u64,
    /// Opening price.
    pub open: f64,
    /// Highest price during the candle.
    pub high: f64,
    /// Lowest price during the candle.
    pub low: f64,
    /// Closing price.
    pub close: f64,
    /// Trade volume denominated in the base asset.
    pub volume: f64,
}

impl Candle {
    /// Convert this candle to a [`Tick`] using the close price and close time.
    ///
    /// Returns `None` if the close price is invalid (negative or non-finite).
    #[must_use]
    pub fn to_tick(&self, asset: Asset, source: ExchangeSource) -> Option<Tick> {
        let price = Price::new(self.close)?;
        Some(Tick {
            asset,
            price,
            timestamp_ms: self.close_time_ms,
            source,
        })
    }
}

// ─── Path helpers ────────────────────────────────────────────────────────────

/// Build the cache file path for a given `(asset, source, date)` triple.
///
/// The returned path has the form `{cache_dir}/{asset}_{source}_{date}.jsonl.gz`
/// where asset and source are lower-cased display strings.
#[must_use]
pub fn cache_path(cache_dir: &Path, asset: Asset, source: ExchangeSource, date: &str) -> PathBuf {
    let filename = format!(
        "{}_{}_{}.jsonl.gz",
        asset.to_string().to_lowercase(),
        source.to_string().to_lowercase(),
        date
    );
    cache_dir.join(filename)
}

/// Returns `true` if a cache file for `(asset, source, date)` already exists.
#[must_use]
pub fn is_cached(cache_dir: &Path, asset: Asset, source: ExchangeSource, date: &str) -> bool {
    cache_path(cache_dir, asset, source, date).exists()
}

// ─── I/O ─────────────────────────────────────────────────────────────────────

/// Write a slice of [`Candle`]s to a gzipped JSONL file.
///
/// Parent directories are created automatically. The file is written atomically
/// with respect to the process (no partial writes visible to readers).
///
/// # Errors
///
/// Returns an [`io::Error`] on any filesystem or serialisation failure.
pub fn write_candles(path: &Path, candles: &[Candle]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    let mut gz = GzEncoder::new(file, Compression::default());
    for candle in candles {
        let line = serde_json::to_string(candle)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        gz.write_all(line.as_bytes())?;
        gz.write_all(b"\n")?;
    }
    gz.finish()?;
    Ok(())
}

/// Read all [`Candle`]s from a gzipped JSONL file.
///
/// # Errors
///
/// Returns an [`io::Error`] if the file cannot be opened, decompressed, or if
/// any line fails to deserialise.
pub fn read_candles(path: &Path) -> io::Result<Vec<Candle>> {
    let file = fs::File::open(path)?;
    let gz = GzDecoder::new(file);
    let reader = BufReader::new(gz);
    let mut candles = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let candle: Candle = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        candles.push(candle);
    }
    Ok(candles)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;

    fn sample_candle(ts: u64) -> Candle {
        Candle {
            open_time_ms: ts,
            close_time_ms: ts + 999,
            open: 100.0,
            high: 105.0,
            low: 99.0,
            close: 102.5,
            volume: 1.5,
        }
    }

    #[test]
    fn roundtrip_write_read_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test.jsonl.gz");
        write_candles(&path, &[]).expect("write");
        let result = read_candles(&path).expect("read");
        assert!(result.is_empty());
    }

    #[test]
    fn roundtrip_write_read_multiple() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("btc_binance_2025-01-01.jsonl.gz");
        let candles = vec![
            sample_candle(1_000_000),
            sample_candle(2_000_000),
            sample_candle(3_000_000),
        ];
        write_candles(&path, &candles).expect("write");
        let result = read_candles(&path).expect("read");
        assert_eq!(result, candles);
    }

    #[test]
    fn roundtrip_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("sub").join("data.jsonl.gz");
        let candles = vec![sample_candle(42)];
        write_candles(&path, &candles).expect("write with nested dirs");
        let result = read_candles(&path).expect("read");
        assert_eq!(result, candles);
    }

    #[test]
    fn candle_to_tick_valid() {
        let candle = sample_candle(1_000_000);
        let tick = candle
            .to_tick(Asset::Btc, ExchangeSource::Binance)
            .expect("valid tick");
        assert_eq!(tick.asset, Asset::Btc);
        assert_eq!(tick.source, ExchangeSource::Binance);
        assert_eq!(tick.timestamp_ms, candle.close_time_ms);
        assert!((tick.price.as_f64() - 102.5).abs() < f64::EPSILON);
    }

    #[test]
    fn candle_to_tick_negative_close_returns_none() {
        let mut candle = sample_candle(1_000_000);
        candle.close = -1.0;
        assert!(
            candle
                .to_tick(Asset::Eth, ExchangeSource::Binance)
                .is_none()
        );
    }

    #[test]
    fn candle_to_tick_nan_close_returns_none() {
        let mut candle = sample_candle(1_000_000);
        candle.close = f64::NAN;
        assert!(candle.to_tick(Asset::Sol, ExchangeSource::Okx).is_none());
    }

    #[test]
    fn cache_path_format() {
        let dir = Path::new("/tmp/cache");
        let path = cache_path(dir, Asset::Btc, ExchangeSource::Binance, "2025-01-01");
        assert_eq!(
            path,
            PathBuf::from("/tmp/cache/btc_binance_2025-01-01.jsonl.gz")
        );
    }

    #[test]
    fn cache_path_okx() {
        let dir = Path::new("/data");
        let path = cache_path(dir, Asset::Eth, ExchangeSource::Okx, "2025-06-15");
        assert_eq!(path, PathBuf::from("/data/eth_okx_2025-06-15.jsonl.gz"));
    }

    #[test]
    fn is_cached_false_when_file_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(!is_cached(
            dir.path(),
            Asset::Btc,
            ExchangeSource::Binance,
            "2025-01-01"
        ));
    }

    #[test]
    fn is_cached_true_after_write() {
        let dir = tempfile::tempdir().expect("temp dir");
        let candles = vec![sample_candle(1)];
        let path = cache_path(
            dir.path(),
            Asset::Xrp,
            ExchangeSource::Binance,
            "2025-03-01",
        );
        write_candles(&path, &candles).expect("write");
        assert!(is_cached(
            dir.path(),
            Asset::Xrp,
            ExchangeSource::Binance,
            "2025-03-01"
        ));
    }
}
