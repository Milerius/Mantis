//! Per-window live data recorder that writes PBT-compatible gzip JSONL files.
//!
//! Each prediction window is buffered in memory and flushed to a compressed
//! file when the window closes (or is resolved). The resulting file has the
//! **exact same format** as the PolyBackTest cache files consumed by
//! [`PbtReplay::load`], making recorded live data directly replayable.
//!
//! File layout: `{data_dir}/live/{coin}_{timeframe}_{window_id}.jsonl.gz`
//! - Line 1: market metadata (same schema as [`PbtMarket`])
//! - Lines 2+: snapshots (same schema as [`PbtSnapshot`])

use std::{
    collections::HashMap,
    fs::{File, create_dir_all},
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use flate2::{Compression, write::GzEncoder};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ─── PBT-compatible wire types ──────────────────────────────────────────────
//
// We define local copies to avoid a dependency on `pm-oracle`. The JSON schema
// is identical to `PbtMarket` / `PbtSnapshot` so that `read_pbt_cache` and
// `PbtReplay::load` can consume the resulting files without any adapter code.

/// Market metadata written as the first line of the gzip JSONL file.
///
/// Field names and types match `pm_oracle::polybacktest::PbtMarket` exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowMarketMeta {
    /// Unique identifier for this window (may be a synthetic ID).
    pub market_id: String,
    /// Human-readable slug.
    pub slug: String,
    /// Market type string (e.g. `"5m"`, `"15m"`).
    pub market_type: String,
    /// ISO-8601 start time.
    pub start_time: String,
    /// ISO-8601 end time.
    pub end_time: String,
    /// Spot price at window open.
    pub btc_price_start: Option<f64>,
    /// Spot price at window close (filled on close).
    pub btc_price_end: Option<f64>,
    /// Resolution outcome: `"Up"`, `"Down"`, or `null`.
    pub winner: Option<String>,
    /// CLOB token ID for the Up outcome (optional).
    pub clob_token_up: Option<String>,
    /// CLOB token ID for the Down outcome (optional).
    pub clob_token_down: Option<String>,
}

/// A snapshot written on each tick, matching `PbtSnapshot` schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSnapshotLine {
    /// ISO-8601 timestamp.
    pub time: String,
    /// Spot price at this moment.
    pub btc_price: Option<f64>,
    /// Best ask for Up contract.
    pub price_up: Option<f64>,
    /// Best ask for Down contract.
    pub price_down: Option<f64>,
}

// ─── In-memory buffer for one window ────────────────────────────────────────

/// Buffered data for a single window before it is flushed to disk.
struct WindowBuffer {
    /// Market metadata (winner is `None` until the window closes).
    meta: WindowMarketMeta,
    /// Pre-serialised JSON snapshot lines.
    snapshot_lines: Vec<String>,
}

// ─── WindowRecorder ─────────────────────────────────────────────────────────

/// Records live data per prediction window in PBT-compatible gzip JSONL files.
///
/// # Usage
///
/// 1. [`open_window`](WindowRecorder::open_window) when a new window starts.
/// 2. [`record_snapshot`](WindowRecorder::record_snapshot) on each tick.
/// 3. [`close_window`](WindowRecorder::close_window) when the window resolves.
///
/// On close the metadata (with winner) and all buffered snapshots are written
/// to `{data_dir}/live/{window_key}.jsonl.gz` in a single pass.
pub struct WindowRecorder {
    /// Active windows keyed by `"{coin}_{timeframe}_{window_id}"`.
    active_windows: HashMap<String, WindowBuffer>,
    /// Base directory for output files (`{data_dir}/live/`).
    live_dir: PathBuf,
}

impl WindowRecorder {
    /// Create a new recorder. Creates `{data_dir}/live/` if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the directory cannot be created.
    pub fn new(data_dir: &Path) -> io::Result<Self> {
        let live_dir = data_dir.join("live");
        create_dir_all(&live_dir)?;
        Ok(Self {
            active_windows: HashMap::new(),
            live_dir,
        })
    }

    /// Build the canonical window key used as both the HashMap key and the
    /// file name stem.
    #[must_use]
    pub fn window_key(coin: &str, timeframe: &str, window_id: &str) -> String {
        format!("{coin}_{timeframe}_{window_id}")
    }

    /// Start recording a new window.
    ///
    /// If a window with the same key already exists it is silently replaced
    /// (the old buffered data is lost — this handles ungraceful restarts).
    pub fn open_window(
        &mut self,
        coin: &str,
        timeframe: &str,
        window_id: &str,
        start_time_iso: &str,
        end_time_iso: &str,
        open_price: f64,
    ) {
        let key = Self::window_key(coin, timeframe, window_id);

        let meta = WindowMarketMeta {
            market_id: window_id.to_owned(),
            slug: format!("{coin}-{timeframe}-{window_id}"),
            market_type: timeframe.to_owned(),
            start_time: start_time_iso.to_owned(),
            end_time: end_time_iso.to_owned(),
            btc_price_start: Some(open_price),
            btc_price_end: None,
            winner: None,
            clob_token_up: None,
            clob_token_down: None,
        };

        if self.active_windows.contains_key(&key) {
            warn!(key = %key, "replacing existing window buffer");
        }

        self.active_windows.insert(
            key.clone(),
            WindowBuffer {
                meta,
                snapshot_lines: Vec::with_capacity(2500),
            },
        );

        debug!(key = %key, "window recording started");
    }

    /// Append a snapshot to the in-memory buffer for `window_key`.
    ///
    /// Returns `Ok(false)` if the window key is unknown (window not open).
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] only if JSON serialization fails (should not
    /// happen in practice).
    pub fn record_snapshot(
        &mut self,
        window_key: &str,
        time_iso: &str,
        spot_price: f64,
        price_up: Option<f64>,
        price_down: Option<f64>,
    ) -> io::Result<bool> {
        let Some(buf) = self.active_windows.get_mut(window_key) else {
            return Ok(false);
        };

        let snap = WindowSnapshotLine {
            time: time_iso.to_owned(),
            btc_price: Some(spot_price),
            price_up,
            price_down,
        };

        let line = serde_json::to_string(&snap)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        buf.snapshot_lines.push(line);
        Ok(true)
    }

    /// Close a window, set the winner and end price, and flush everything to
    /// a compressed file.
    ///
    /// Returns the path of the written file on success, or `Ok(None)` if the
    /// window key was not found (already closed or never opened).
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] on serialisation or file I/O failures.
    pub fn close_window(
        &mut self,
        window_key: &str,
        winner: &str,
        end_price: f64,
    ) -> io::Result<Option<PathBuf>> {
        let Some(mut buf) = self.active_windows.remove(window_key) else {
            return Ok(None);
        };

        buf.meta.winner = Some(winner.to_owned());
        buf.meta.btc_price_end = Some(end_price);

        let file_path = self.live_dir.join(format!("{window_key}.jsonl.gz"));
        let file = File::create(&file_path)?;
        let mut gz = GzEncoder::new(file, Compression::fast());

        // Line 1: market metadata.
        let meta_json = serde_json::to_string(&buf.meta)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        gz.write_all(meta_json.as_bytes())?;
        gz.write_all(b"\n")?;

        // Lines 2+: snapshots.
        for line in &buf.snapshot_lines {
            gz.write_all(line.as_bytes())?;
            gz.write_all(b"\n")?;
        }

        gz.finish()?;

        info!(
            key = %window_key,
            snapshots = buf.snapshot_lines.len(),
            winner = %winner,
            path = %file_path.display(),
            "window recording flushed"
        );

        Ok(Some(file_path))
    }

    /// Number of currently active (open) windows.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active_windows.len()
    }

    /// Check whether a window key is currently being recorded.
    #[must_use]
    pub fn is_active(&self, window_key: &str) -> bool {
        self.active_windows.contains_key(window_key)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::{BufRead as _, BufReader, Read as _};

    /// Read a gzip JSONL file and return all lines.
    fn read_gz_lines(path: &Path) -> Vec<String> {
        let file = File::open(path).expect("file should exist");
        let reader = BufReader::new(GzDecoder::new(file));
        reader
            .lines()
            .map(|l| l.expect("valid utf-8"))
            .filter(|l| !l.trim().is_empty())
            .collect()
    }

    #[test]
    fn window_key_format() {
        assert_eq!(
            WindowRecorder::window_key("btc", "5m", "abc123"),
            "btc_5m_abc123"
        );
    }

    #[test]
    fn open_record_close_roundtrip() {
        let dir = std::env::temp_dir().join("pm_winrec_roundtrip");
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut rec = WindowRecorder::new(&dir).expect("create recorder");
        let key = WindowRecorder::window_key("btc", "5m", "w001");

        rec.open_window(
            "btc",
            "5m",
            "w001",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:05:00Z",
            95000.0,
        );
        assert!(rec.is_active(&key));
        assert_eq!(rec.active_count(), 1);

        rec.record_snapshot(&key, "2026-01-01T00:01:00Z", 95050.0, Some(0.52), Some(0.49))
            .expect("record 1");
        rec.record_snapshot(&key, "2026-01-01T00:02:00Z", 95100.0, Some(0.55), Some(0.46))
            .expect("record 2");

        let path = rec
            .close_window(&key, "Up", 95100.0)
            .expect("close")
            .expect("path");

        assert!(!rec.is_active(&key));
        assert_eq!(rec.active_count(), 0);

        // Verify file contents.
        let lines = read_gz_lines(&path);
        assert_eq!(lines.len(), 3, "1 meta + 2 snapshots");

        // Parse metadata.
        let meta: WindowMarketMeta =
            serde_json::from_str(&lines[0]).expect("parse meta");
        assert_eq!(meta.market_id, "w001");
        assert_eq!(meta.market_type, "5m");
        assert_eq!(meta.winner, Some("Up".to_owned()));
        assert!((meta.btc_price_start.expect("start") - 95000.0).abs() < 1e-6);
        assert!((meta.btc_price_end.expect("end") - 95100.0).abs() < 1e-6);

        // Parse snapshot.
        let snap: WindowSnapshotLine =
            serde_json::from_str(&lines[1]).expect("parse snap");
        assert_eq!(snap.time, "2026-01-01T00:01:00Z");
        assert!((snap.btc_price.expect("price") - 95050.0).abs() < 1e-6);
        assert!((snap.price_up.expect("up") - 0.52).abs() < 1e-6);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_unknown_window_returns_none() {
        let dir = std::env::temp_dir().join("pm_winrec_unknown");
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut rec = WindowRecorder::new(&dir).expect("create recorder");
        let result = rec
            .close_window("nonexistent", "Up", 100.0)
            .expect("should not error");
        assert!(result.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_to_unknown_window_returns_false() {
        let dir = std::env::temp_dir().join("pm_winrec_norec");
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut rec = WindowRecorder::new(&dir).expect("create recorder");
        let recorded = rec
            .record_snapshot("nonexistent", "2026-01-01T00:00:00Z", 100.0, None, None)
            .expect("should not error");
        assert!(!recorded);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pbt_replay_compatible_format() {
        // Verify that the output file can be read by read_pbt_cache-compatible
        // logic: line 1 is valid PbtMarket JSON, lines 2+ are valid PbtSnapshot
        // JSON (with optional fields defaulting).
        let dir = std::env::temp_dir().join("pm_winrec_pbt_compat");
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut rec = WindowRecorder::new(&dir).expect("create recorder");
        let key = WindowRecorder::window_key("btc", "15m", "pbt001");

        rec.open_window(
            "btc",
            "15m",
            "pbt001",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:15:00Z",
            95000.0,
        );

        for i in 0..5 {
            let t = format!("2026-01-01T00:{:02}:00Z", i * 3);
            let price = 95000.0 + f64::from(i) * 30.0;
            rec.record_snapshot(&key, &t, price, Some(0.50 + 0.01 * f64::from(i)), Some(0.51 - 0.01 * f64::from(i)))
                .expect("record");
        }

        let path = rec
            .close_window(&key, "Up", 95120.0)
            .expect("close")
            .expect("path");

        let lines = read_gz_lines(&path);
        assert_eq!(lines.len(), 6); // 1 meta + 5 snaps

        // Ensure the metadata line has the required PbtMarket fields.
        let meta: serde_json::Value = serde_json::from_str(&lines[0]).expect("parse meta json");
        assert!(meta.get("market_id").is_some());
        assert!(meta.get("slug").is_some());
        assert!(meta.get("market_type").is_some());
        assert!(meta.get("start_time").is_some());
        assert!(meta.get("end_time").is_some());
        assert!(meta.get("btc_price_start").is_some());
        assert!(meta.get("winner").is_some());

        // Ensure snapshot lines have the required PbtSnapshot fields.
        let snap: serde_json::Value = serde_json::from_str(&lines[1]).expect("parse snap json");
        assert!(snap.get("time").is_some());
        assert!(snap.get("btc_price").is_some());
        assert!(snap.get("price_up").is_some());
        assert!(snap.get("price_down").is_some());

        std::fs::remove_dir_all(&dir).ok();
    }
}
