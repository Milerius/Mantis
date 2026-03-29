//! Live market data recorder — compresses JSONL to disk for future replay.
//!
//! [`LiveRecorder`] writes two gzip-compressed JSONL streams: one for spot
//! price ticks and one for orderbook snapshots. Files are flushed explicitly
//! via [`LiveRecorder::flush`].
//!
//! Files are written to:
//! - `{data_dir}/live/{session_id}_ticks.jsonl.gz`
//! - `{data_dir}/live/{session_id}_orderbook.jsonl.gz`

use std::{
    fs::{File, create_dir_all},
    io::{self, BufWriter, Write},
    path::Path,
};

use flate2::Compression;
use flate2::write::GzEncoder;
use pm_types::Tick;
use serde::{Deserialize, Serialize};

// ─── Wire types ───────────────────────────────────────────────────────────────

/// A single orderbook snapshot line written to the JSONL file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderbookLine {
    /// The Polymarket condition/window identifier.
    pub window_id: String,
    /// Timestamp in milliseconds since Unix epoch.
    pub timestamp_ms: u64,
    /// Ask price for the Up contract.
    pub ask_up: f64,
    /// Ask price for the Down contract.
    pub ask_down: f64,
    /// Bid price for the Up contract.
    pub bid_up: f64,
    /// Bid price for the Down contract.
    pub bid_down: f64,
}

// ─── LiveRecorder ─────────────────────────────────────────────────────────────

/// Records live market data to disk for future replay.
///
/// Tick data and orderbook snapshots are written to separate gzip-compressed
/// JSONL files. Each line is a valid JSON object.
pub struct LiveRecorder {
    tick_writer: BufWriter<GzEncoder<File>>,
    orderbook_writer: BufWriter<GzEncoder<File>>,
}

impl LiveRecorder {
    /// Create a new [`LiveRecorder`] for the given session.
    ///
    /// Creates `{data_dir}/live/` if it does not exist, then opens:
    /// - `{data_dir}/live/{session_id}_ticks.jsonl.gz`
    /// - `{data_dir}/live/{session_id}_orderbook.jsonl.gz`
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if directories cannot be created or files cannot
    /// be opened.
    pub fn new(data_dir: &Path, session_id: &str) -> io::Result<Self> {
        let live_dir = data_dir.join("live");
        create_dir_all(&live_dir)?;

        let tick_path = live_dir.join(format!("{session_id}_ticks.jsonl.gz"));
        let ob_path = live_dir.join(format!("{session_id}_orderbook.jsonl.gz"));

        let tick_file = File::create(tick_path)?;
        let ob_file = File::create(ob_path)?;

        let tick_gz = GzEncoder::new(tick_file, Compression::default());
        let ob_gz = GzEncoder::new(ob_file, Compression::default());

        Ok(Self {
            tick_writer: BufWriter::new(tick_gz),
            orderbook_writer: BufWriter::new(ob_gz),
        })
    }

    /// Record a spot price tick.
    ///
    /// Serialises `tick` as a JSONL line and writes it to the tick file.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if serialisation or write fails.
    pub fn record_tick(&mut self, tick: &Tick) -> io::Result<()> {
        let line = serde_json::to_string(tick)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.tick_writer.write_all(line.as_bytes())?;
        self.tick_writer.write_all(b"\n")
    }

    /// Record an orderbook snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if serialisation or write fails.
    #[expect(
        clippy::too_many_arguments,
        reason = "all args are independent fields of an orderbook snapshot; grouping into a struct would add indirection for a single call site"
    )]
    pub fn record_orderbook(
        &mut self,
        window_id: &str,
        timestamp_ms: u64,
        ask_up: f64,
        ask_down: f64,
        bid_up: f64,
        bid_down: f64,
    ) -> io::Result<()> {
        let line_val = OrderbookLine {
            window_id: window_id.to_owned(),
            timestamp_ms,
            ask_up,
            ask_down,
            bid_up,
            bid_down,
        };
        let line = serde_json::to_string(&line_val)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.orderbook_writer.write_all(line.as_bytes())?;
        self.orderbook_writer.write_all(b"\n")
    }

    /// Flush all writers to their underlying gzip streams.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if either flush fails.
    pub fn flush(&mut self) -> io::Result<()> {
        self.tick_writer.flush()?;
        self.orderbook_writer.flush()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use std::io::Read as _;

    use flate2::read::GzDecoder;
    use pm_types::{Asset, ExchangeSource, Price};

    use super::*;

    fn make_tick(asset: Asset, price: f64, timestamp_ms: u64) -> Tick {
        Tick {
            asset,
            price: Price::new(price).expect("valid price"),
            timestamp_ms,
            source: ExchangeSource::Binance,
        }
    }

    /// Read and decompress a `.jsonl.gz` file into lines.
    fn read_gz_lines(path: &Path) -> Vec<String> {
        let file = File::open(path).expect("file should exist");
        let mut gz = GzDecoder::new(file);
        let mut content = String::new();
        gz.read_to_string(&mut content).expect("decompress should succeed");
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect()
    }

    // ── Test 1: Tick roundtrip ────────────────────────────────────────────────

    #[test]
    fn tick_write_read_roundtrip() {
        let dir = std::env::temp_dir().join("pm_recorder_test_tick_roundtrip");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let session = "test_session_tick";
        let mut recorder = LiveRecorder::new(&dir, session).expect("create recorder");

        let tick1 = make_tick(Asset::Btc, 84_000.0, 1_000_000);
        let tick2 = make_tick(Asset::Eth, 3_200.0, 2_000_000);

        recorder.record_tick(&tick1).expect("record tick1");
        recorder.record_tick(&tick2).expect("record tick2");
        recorder.flush().expect("flush");
        drop(recorder);

        let tick_path = dir.join("live").join(format!("{session}_ticks.jsonl.gz"));
        let lines = read_gz_lines(&tick_path);
        assert_eq!(lines.len(), 2, "expected 2 tick lines");

        let parsed1: Tick = serde_json::from_str(&lines[0]).expect("parse tick1");
        let parsed2: Tick = serde_json::from_str(&lines[1]).expect("parse tick2");

        assert_eq!(parsed1.asset, tick1.asset);
        assert!((parsed1.price.as_f64() - tick1.price.as_f64()).abs() < 1e-9);
        assert_eq!(parsed1.timestamp_ms, tick1.timestamp_ms);
        assert_eq!(parsed1.source, tick1.source);

        assert_eq!(parsed2.asset, tick2.asset);
        assert!((parsed2.price.as_f64() - tick2.price.as_f64()).abs() < 1e-9);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Test 2: Orderbook roundtrip ───────────────────────────────────────────

    #[test]
    fn orderbook_write_read_roundtrip() {
        let dir = std::env::temp_dir().join("pm_recorder_test_ob_roundtrip");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let session = "test_session_ob";
        let mut recorder = LiveRecorder::new(&dir, session).expect("create recorder");

        recorder
            .record_orderbook("window_42", 1_500_000, 0.60, 0.43, 0.58, 0.41)
            .expect("record orderbook");
        recorder.flush().expect("flush");
        drop(recorder);

        let ob_path = dir
            .join("live")
            .join(format!("{session}_orderbook.jsonl.gz"));
        let lines = read_gz_lines(&ob_path);
        assert_eq!(lines.len(), 1, "expected 1 orderbook line");

        let parsed: OrderbookLine =
            serde_json::from_str(&lines[0]).expect("parse orderbook line");

        assert_eq!(parsed.window_id, "window_42");
        assert_eq!(parsed.timestamp_ms, 1_500_000);
        assert!((parsed.ask_up - 0.60).abs() < 1e-10);
        assert!((parsed.ask_down - 0.43).abs() < 1e-10);
        assert!((parsed.bid_up - 0.58).abs() < 1e-10);
        assert!((parsed.bid_down - 0.41).abs() < 1e-10);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Test 3: Multiple sessions don't interfere ────────────────────────────

    #[test]
    fn separate_sessions_write_separate_files() {
        let dir = std::env::temp_dir().join("pm_recorder_test_sessions");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let mut rec_a = LiveRecorder::new(&dir, "session_a").expect("create rec_a");
        let mut rec_b = LiveRecorder::new(&dir, "session_b").expect("create rec_b");

        rec_a
            .record_tick(&make_tick(Asset::Btc, 50_000.0, 1_000))
            .expect("record a");
        rec_b
            .record_tick(&make_tick(Asset::Eth, 3_000.0, 2_000))
            .expect("record b");
        rec_a.flush().expect("flush a");
        rec_b.flush().expect("flush b");
        drop(rec_a);
        drop(rec_b);

        let path_a = dir.join("live").join("session_a_ticks.jsonl.gz");
        let path_b = dir.join("live").join("session_b_ticks.jsonl.gz");

        let lines_a = read_gz_lines(&path_a);
        let lines_b = read_gz_lines(&path_b);

        assert_eq!(lines_a.len(), 1);
        assert_eq!(lines_b.len(), 1);

        let t_a: Tick = serde_json::from_str(&lines_a[0]).expect("parse a");
        let t_b: Tick = serde_json::from_str(&lines_b[0]).expect("parse b");

        assert_eq!(t_a.asset, Asset::Btc);
        assert_eq!(t_b.asset, Asset::Eth);

        std::fs::remove_dir_all(&dir).ok();
    }
}
