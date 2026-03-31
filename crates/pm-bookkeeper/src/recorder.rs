//! Live market data recorder — writes combined JSONL snapshots to disk for future replay.
//!
//! [`SnapshotRecorder`] writes one plain JSONL file per session containing
//! combined spot + orderbook snapshots. Files are flushed after every
//! [`FLUSH_INTERVAL`] writes to bound worst-case data loss without adding
//! time-based complexity.
//!
//! File is written to:
//! - `{data_dir}/live/{session_id}_snapshots.jsonl`

use std::{
    fs::{File, create_dir_all},
    io::{self, BufWriter, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Number of writes between automatic flushes.
const FLUSH_INTERVAL: usize = 10;

// ─── Wire types ───────────────────────────────────────────────────────────────

/// A combined spot + orderbook snapshot written to the JSONL file.
///
/// This format is compatible with what the backtest replay system needs:
/// spot price, contract prices, and window metadata in a single record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveSnapshot {
    /// Timestamp in milliseconds since Unix epoch.
    pub time_ms: u64,
    /// Asset symbol (e.g. `"BTC"`).
    pub asset: String,
    /// Spot price at this snapshot.
    pub spot_price: f64,
    /// Best ask for the Up contract, if available.
    pub price_up: Option<f64>,
    /// Best ask for the Down contract, if available.
    pub price_down: Option<f64>,
    /// Best bid for the Up contract, if available.
    pub bid_up: Option<f64>,
    /// Best bid for the Down contract, if available.
    pub bid_down: Option<f64>,
    /// Window open timestamp in milliseconds since Unix epoch.
    pub window_open_ms: u64,
    /// Window duration in seconds.
    pub timeframe_secs: u64,
}

// ─── SnapshotRecorder ─────────────────────────────────────────────────────────

/// Records live combined spot + orderbook snapshots to a plain JSONL file.
///
/// One file per session; flushed after every [`FLUSH_INTERVAL`] writes.
/// No gzip — files can be compressed offline; reliability takes priority.
pub struct SnapshotRecorder {
    writer: BufWriter<File>,
    writes_since_flush: usize,
}

impl SnapshotRecorder {
    /// Create a new [`SnapshotRecorder`] for the given session.
    ///
    /// Creates `{data_dir}/live/` if it does not exist, then opens:
    /// - `{data_dir}/live/{session_id}_snapshots.jsonl`
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if directories cannot be created or the file
    /// cannot be opened.
    pub fn new(data_dir: &Path, session_id: &str) -> io::Result<Self> {
        let live_dir = data_dir.join("live");
        create_dir_all(&live_dir)?;

        let snap_path = live_dir.join(format!("{session_id}_snapshots.jsonl"));
        let file = File::create(snap_path)?;

        Ok(Self {
            writer: BufWriter::new(file),
            writes_since_flush: 0,
        })
    }

    /// Record a combined snapshot.
    ///
    /// Flushes automatically every [`FLUSH_INTERVAL`] writes.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if serialisation, write, or flush fails.
    #[expect(
        clippy::too_many_arguments,
        reason = "all args are independent snapshot fields; grouping would add indirection for a single call site"
    )]
    pub fn record(
        &mut self,
        time_ms: u64,
        asset: &str,
        spot_price: f64,
        price_up: Option<f64>,
        price_down: Option<f64>,
        bid_up: Option<f64>,
        bid_down: Option<f64>,
        window_open_ms: u64,
        timeframe_secs: u64,
    ) -> io::Result<()> {
        let snap = LiveSnapshot {
            time_ms,
            asset: asset.to_owned(),
            spot_price,
            price_up,
            price_down,
            bid_up,
            bid_down,
            window_open_ms,
            timeframe_secs,
        };

        let line = serde_json::to_string(&snap)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;

        self.writes_since_flush += 1;
        if self.writes_since_flush >= FLUSH_INTERVAL {
            self.writer.flush()?;
            self.writes_since_flush = 0;
        }

        Ok(())
    }

    /// Flush any buffered data to disk.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the flush fails.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writes_since_flush = 0;
        self.writer.flush()
    }
}

// ─── LiveRecorder (compatibility alias) ─────────────────────────────────────

/// Deprecated: use [`SnapshotRecorder`] instead.
///
/// This alias exists only to keep old call sites compiling during migration.
/// Remove once all uses are updated.
pub type LiveRecorder = SnapshotRecorder;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use std::io::BufRead as _;

    use super::*;

    fn read_jsonl_lines(path: &Path) -> Vec<String> {
        let file = File::open(path).expect("file should exist");
        let reader = std::io::BufReader::new(file);
        reader
            .lines()
            .map(|l| l.expect("line should be valid utf8"))
            .filter(|l| !l.trim().is_empty())
            .collect()
    }

    // ── Test 1: Single snapshot roundtrip ────────────────────────────────────

    #[test]
    fn snapshot_write_read_roundtrip() {
        let dir = std::env::temp_dir().join("pm_recorder_test_snap_roundtrip");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let session = "test_snap_session";
        let mut recorder = SnapshotRecorder::new(&dir, session).expect("create recorder");

        recorder
            .record(
                1_000_000,
                "BTC",
                84_000.0,
                Some(0.55),
                Some(0.46),
                Some(0.53),
                Some(0.44),
                999_000,
                900,
            )
            .expect("record snapshot");
        recorder.flush().expect("flush");
        drop(recorder);

        let snap_path = dir
            .join("live")
            .join(format!("{session}_snapshots.jsonl"));
        let lines = read_jsonl_lines(&snap_path);
        assert_eq!(lines.len(), 1, "expected 1 snapshot line");

        let parsed: LiveSnapshot = serde_json::from_str(&lines[0]).expect("parse snapshot");
        assert_eq!(parsed.time_ms, 1_000_000);
        assert_eq!(parsed.asset, "BTC");
        assert!((parsed.spot_price - 84_000.0).abs() < 1e-9);
        assert_eq!(parsed.price_up, Some(0.55));
        assert_eq!(parsed.price_down, Some(0.46));
        assert_eq!(parsed.bid_up, Some(0.53));
        assert_eq!(parsed.bid_down, Some(0.44));
        assert_eq!(parsed.window_open_ms, 999_000);
        assert_eq!(parsed.timeframe_secs, 900);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Test 2: Multiple snapshots, auto-flush boundary ──────────────────────

    #[test]
    fn multiple_snapshots_auto_flush_at_interval() {
        let dir = std::env::temp_dir().join("pm_recorder_test_multi_snap");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let session = "test_multi_snap";
        let mut recorder = SnapshotRecorder::new(&dir, session).expect("create recorder");

        // Write exactly FLUSH_INTERVAL snapshots to trigger one auto-flush.
        for i in 0..10_u64 {
            recorder
                .record(i * 1_000, "ETH", 3_200.0 + i as f64, None, None, None, None, 0, 300)
                .expect("record snapshot");
        }
        // Final explicit flush.
        recorder.flush().expect("flush");
        drop(recorder);

        let snap_path = dir
            .join("live")
            .join(format!("{session}_snapshots.jsonl"));
        let lines = read_jsonl_lines(&snap_path);
        assert_eq!(lines.len(), 10, "expected 10 snapshot lines");

        // Spot-check first and last.
        let first: LiveSnapshot = serde_json::from_str(&lines[0]).expect("parse first");
        let last: LiveSnapshot = serde_json::from_str(&lines[9]).expect("parse last");
        assert_eq!(first.time_ms, 0);
        assert_eq!(last.time_ms, 9_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Test 3: Null orderbook fields ────────────────────────────────────────

    #[test]
    fn snapshot_with_no_orderbook_data() {
        let dir = std::env::temp_dir().join("pm_recorder_test_no_ob");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let session = "test_no_ob";
        let mut recorder = SnapshotRecorder::new(&dir, session).expect("create recorder");

        recorder
            .record(5_000_000, "SOL", 145.0, None, None, None, None, 4_995_000, 300)
            .expect("record snapshot without orderbook");
        recorder.flush().expect("flush");
        drop(recorder);

        let snap_path = dir
            .join("live")
            .join(format!("{session}_snapshots.jsonl"));
        let lines = read_jsonl_lines(&snap_path);
        assert_eq!(lines.len(), 1);

        let parsed: LiveSnapshot = serde_json::from_str(&lines[0]).expect("parse snapshot");
        assert!(parsed.price_up.is_none());
        assert!(parsed.price_down.is_none());
        assert!(parsed.bid_up.is_none());
        assert!(parsed.bid_down.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Test 4: Separate sessions don't interfere ─────────────────────────────

    #[test]
    fn separate_sessions_write_separate_files() {
        let dir = std::env::temp_dir().join("pm_recorder_test_separate");
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let mut rec_a = SnapshotRecorder::new(&dir, "sess_a").expect("create rec_a");
        let mut rec_b = SnapshotRecorder::new(&dir, "sess_b").expect("create rec_b");

        rec_a
            .record(1_000, "BTC", 50_000.0, Some(0.55), Some(0.46), None, None, 0, 900)
            .expect("record a");
        rec_b
            .record(2_000, "ETH", 3_000.0, None, None, None, None, 0, 300)
            .expect("record b");

        rec_a.flush().expect("flush a");
        rec_b.flush().expect("flush b");
        drop(rec_a);
        drop(rec_b);

        let path_a = dir.join("live").join("sess_a_snapshots.jsonl");
        let path_b = dir.join("live").join("sess_b_snapshots.jsonl");

        let lines_a = read_jsonl_lines(&path_a);
        let lines_b = read_jsonl_lines(&path_b);
        assert_eq!(lines_a.len(), 1);
        assert_eq!(lines_b.len(), 1);

        let snap_a: LiveSnapshot = serde_json::from_str(&lines_a[0]).expect("parse a");
        let snap_b: LiveSnapshot = serde_json::from_str(&lines_b[0]).expect("parse b");
        assert_eq!(snap_a.asset, "BTC");
        assert_eq!(snap_b.asset, "ETH");

        std::fs::remove_dir_all(&dir).ok();
    }
}
