//! Historical replay iterator.
//!
//! [`HistoricalReplay`] loads cached candle data for one or more assets and
//! dates, converts every candle to a [`Tick`], sorts the whole collection by
//! timestamp, and then exposes the result as a standard [`Iterator`].  This
//! makes it trivial to feed the signal engine with deterministic historical
//! data during backtesting.

use std::{io, path::Path};

use pm_types::{Asset, ExchangeSource, Tick};

use crate::storage;

// ─── HistoricalReplay ────────────────────────────────────────────────────────

/// A time-sorted iterator over historical [`Tick`]s loaded from the candle
/// cache.
///
/// Construct with [`HistoricalReplay::load`], then iterate with the standard
/// [`Iterator`] interface.  Call [`HistoricalReplay::reset`] to replay the
/// same data from the beginning.
pub struct HistoricalReplay {
    ticks: Vec<Tick>,
    cursor: usize,
}

impl HistoricalReplay {
    /// Load candles for every `(asset, date)` pair from `cache_dir`, convert
    /// them to [`Tick`]s, and sort the full collection by timestamp.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if any expected cache file is missing or
    /// cannot be read.
    pub fn load(
        cache_dir: &Path,
        assets: &[Asset],
        source: ExchangeSource,
        dates: &[String],
    ) -> io::Result<Self> {
        let mut ticks: Vec<Tick> = Vec::new();

        for &asset in assets {
            for date in dates {
                let path = storage::cache_path(cache_dir, asset, source, date);
                let candles = storage::read_candles(&path)?;
                for candle in candles {
                    if let Some(tick) = candle.to_tick(asset, source) {
                        ticks.push(tick);
                    }
                }
            }
        }

        ticks.sort_unstable_by_key(|t| t.timestamp_ms);

        Ok(Self { ticks, cursor: 0 })
    }

    /// Number of ticks available in total (before and after the cursor).
    #[must_use]
    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    /// Returns `true` if no ticks were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// Reset the cursor to the beginning so the same data can be replayed
    /// again without reloading from disk.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }
}

impl Iterator for HistoricalReplay {
    type Item = Tick;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor < self.ticks.len() {
            let tick = self.ticks[self.cursor];
            self.cursor += 1;
            Some(tick)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.ticks.len() - self.cursor;
        (remaining, Some(remaining))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;
    use crate::storage::{Candle, write_candles};

    fn make_candle(ts_ms: u64, close: f64) -> Candle {
        Candle {
            open_time_ms: ts_ms,
            close_time_ms: ts_ms + 999,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
        }
    }

    #[test]
    fn replay_yields_ticks_in_order() {
        let dir = tempfile::tempdir().expect("temp dir");
        let date = "2025-01-01";

        // Write BTC candles out of order to verify sort.
        let btc_candles = vec![
            make_candle(3_000, 30_000.0),
            make_candle(1_000, 10_000.0),
            make_candle(2_000, 20_000.0),
        ];
        let btc_path = storage::cache_path(dir.path(), Asset::Btc, ExchangeSource::Binance, date);
        write_candles(&btc_path, &btc_candles).expect("write btc");

        let mut replay = HistoricalReplay::load(
            dir.path(),
            &[Asset::Btc],
            ExchangeSource::Binance,
            &[date.to_string()],
        )
        .expect("load");

        assert_eq!(replay.len(), 3);
        assert!(!replay.is_empty());

        let t1 = replay.next().expect("tick 1");
        let t2 = replay.next().expect("tick 2");
        let t3 = replay.next().expect("tick 3");
        assert!(t1.timestamp_ms <= t2.timestamp_ms);
        assert!(t2.timestamp_ms <= t3.timestamp_ms);
        assert!(replay.next().is_none());
    }

    #[test]
    fn replay_multiple_assets_sorted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let date = "2025-01-01";

        // BTC ticks at 1000, 3000 ms
        let btc = vec![make_candle(1_000, 50_000.0), make_candle(3_000, 51_000.0)];
        // ETH ticks at 2000, 4000 ms
        let eth = vec![make_candle(2_000, 3_000.0), make_candle(4_000, 3_100.0)];

        write_candles(
            &storage::cache_path(dir.path(), Asset::Btc, ExchangeSource::Binance, date),
            &btc,
        )
        .expect("write btc");
        write_candles(
            &storage::cache_path(dir.path(), Asset::Eth, ExchangeSource::Binance, date),
            &eth,
        )
        .expect("write eth");

        let replay = HistoricalReplay::load(
            dir.path(),
            &[Asset::Btc, Asset::Eth],
            ExchangeSource::Binance,
            &[date.to_string()],
        )
        .expect("load");

        assert_eq!(replay.len(), 4);

        let timestamps: Vec<u64> = replay.map(|t| t.timestamp_ms).collect();
        let mut sorted = timestamps.clone();
        sorted.sort_unstable();
        assert_eq!(timestamps, sorted, "ticks must be in ascending order");
    }

    #[test]
    fn replay_missing_cache_returns_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let result = HistoricalReplay::load(
            dir.path(),
            &[Asset::Btc],
            ExchangeSource::Binance,
            &["2025-01-01".to_string()],
        );
        assert!(result.is_err(), "missing cache should return io error");
    }

    #[test]
    fn replay_reset_replays_from_start() {
        let dir = tempfile::tempdir().expect("temp dir");
        let date = "2025-06-01";
        let candles = vec![
            make_candle(100, 1.0),
            make_candle(200, 2.0),
            make_candle(300, 3.0),
        ];
        let path = storage::cache_path(dir.path(), Asset::Sol, ExchangeSource::Binance, date);
        write_candles(&path, &candles).expect("write");

        let mut replay = HistoricalReplay::load(
            dir.path(),
            &[Asset::Sol],
            ExchangeSource::Binance,
            &[date.to_string()],
        )
        .expect("load");

        // First pass.
        let first_pass: Vec<Tick> = replay.by_ref().collect();
        assert_eq!(first_pass.len(), 3);
        assert!(replay.next().is_none());

        // Reset and replay.
        replay.reset();
        let second_pass: Vec<Tick> = replay.collect();
        assert_eq!(second_pass.len(), 3);
        assert_eq!(
            first_pass
                .iter()
                .map(|t| t.timestamp_ms)
                .collect::<Vec<_>>(),
            second_pass
                .iter()
                .map(|t| t.timestamp_ms)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn replay_empty_when_no_candles() {
        let dir = tempfile::tempdir().expect("temp dir");
        let date = "2025-01-01";
        let path = storage::cache_path(dir.path(), Asset::Xrp, ExchangeSource::Binance, date);
        write_candles(&path, &[]).expect("write empty");

        let replay = HistoricalReplay::load(
            dir.path(),
            &[Asset::Xrp],
            ExchangeSource::Binance,
            &[date.to_string()],
        )
        .expect("load");

        assert!(replay.is_empty());
        assert_eq!(replay.len(), 0);
    }

    #[test]
    fn replay_multiple_dates() {
        let dir = tempfile::tempdir().expect("temp dir");

        let days = ["2025-01-01", "2025-01-02"];
        for (i, day) in days.iter().enumerate() {
            let candles = vec![make_candle((i as u64 + 1) * 1_000, 100.0)];
            let path = storage::cache_path(dir.path(), Asset::Btc, ExchangeSource::Binance, day);
            write_candles(&path, &candles).expect("write");
        }

        let dates: Vec<String> = days.iter().map(|s| s.to_string()).collect();
        let replay =
            HistoricalReplay::load(dir.path(), &[Asset::Btc], ExchangeSource::Binance, &dates)
                .expect("load");

        assert_eq!(replay.len(), 2);
    }
}
