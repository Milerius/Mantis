//! Append-only JSONL trade log writer and reader.
//!
//! Each call to [`TradeLog::append`] serialises one [`TradeRecord`] as a JSON
//! object followed by a newline (JSONL format) and flushes to disk immediately.

use std::{
    fs::{File, OpenOptions, create_dir_all},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use pm_types::TradeRecord;

// ─── TradeLog ────────────────────────────────────────────────────────────────

/// Append-only JSONL writer for [`TradeRecord`] values.
///
/// Each record is written as a single JSON line followed by `\n`.  The file is
/// opened in append mode so existing records are preserved across restarts.
pub struct TradeLog {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl TradeLog {
    /// Open (or create) the trade log at `path`.
    ///
    /// Parent directories are created automatically.  The file is opened in
    /// append mode so that existing records are never overwritten.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if the parent directory cannot be created or the
    /// file cannot be opened.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
        })
    }

    /// Append one [`TradeRecord`] to the log and flush immediately.
    ///
    /// # Errors
    ///
    /// Returns [`io::Error`] if serialisation or the write/flush fails.
    pub fn append(&mut self, record: &TradeRecord) -> io::Result<()> {
        let line = serde_json::to_string(record)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()
    }

    /// Return the path this log writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ─── read_trade_log ──────────────────────────────────────────────────────────

/// Read all [`TradeRecord`] entries from a JSONL file at `path`.
///
/// Empty lines are skipped.  Returns an error if any non-empty line fails to
/// deserialise.
///
/// # Errors
///
/// Returns [`io::Error`] if the file cannot be opened or a line cannot be
/// deserialised.
pub fn read_trade_log(path: &Path) -> io::Result<Vec<TradeRecord>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: TradeRecord = serde_json::from_str(trimmed)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        records.push(record);
    }

    Ok(records)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use pm_types::{Asset, ContractPrice, OrderReason, Pnl, Side, WindowId};

    use super::*;

    fn make_record(window: u64, pnl: f64) -> TradeRecord {
        TradeRecord {
            window_id: WindowId::new(window),
            asset: Asset::Btc,
            side: Side::Up,
            entry_price: ContractPrice::new(0.45).expect("valid entry"),
            exit_price: ContractPrice::new(0.80).expect("valid exit"),
            size_usdc: 25.0,
            pnl: Pnl::new(pnl).expect("finite pnl"),
            opened_at_ms: 0,
            closed_at_ms: 3_600_000,
            close_reason: OrderReason::ExpiryClose,
        }
    }

    #[test]
    fn roundtrip_empty_log() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_test_roundtrip_empty");
        let path = dir.join("trades.jsonl");
        let _ = std::fs::remove_file(&path);

        let mut log = TradeLog::open(&path).expect("open");
        drop(log);

        // Re-open to confirm path works
        log = TradeLog::open(&path).expect("re-open");
        assert_eq!(log.path(), path.as_path());

        let records = read_trade_log(&path).expect("read");
        assert!(records.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn roundtrip_single_record() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_test_roundtrip_single");
        let path = dir.join("trades.jsonl");
        let _ = std::fs::remove_file(&path);

        let original = make_record(1, 8.75);
        let mut log = TradeLog::open(&path).expect("open");
        log.append(&original).expect("append");

        let records = read_trade_log(&path).expect("read");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].window_id, original.window_id);
        assert_eq!(records[0].pnl.as_f64(), original.pnl.as_f64());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn roundtrip_multiple_records() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_test_roundtrip_multi");
        let path = dir.join("trades.jsonl");
        let _ = std::fs::remove_file(&path);

        let originals = vec![
            make_record(1, 8.75),
            make_record(2, -3.50),
            make_record(3, 0.0),
        ];

        let mut log = TradeLog::open(&path).expect("open");
        for r in &originals {
            log.append(r).expect("append");
        }
        drop(log);

        let records = read_trade_log(&path).expect("read");
        assert_eq!(records.len(), originals.len());
        for (got, expected) in records.iter().zip(originals.iter()) {
            assert_eq!(got.window_id, expected.window_id);
            assert_eq!(got.pnl.as_f64(), expected.pnl.as_f64());
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_persists_across_reopen() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_test_persist");
        let path = dir.join("trades.jsonl");
        let _ = std::fs::remove_file(&path);

        let r1 = make_record(10, 5.0);
        let r2 = make_record(11, -2.0);

        // First session
        let mut log = TradeLog::open(&path).expect("open first");
        log.append(&r1).expect("append r1");
        drop(log);

        // Second session
        let mut log = TradeLog::open(&path).expect("open second");
        log.append(&r2).expect("append r2");
        drop(log);

        let records = read_trade_log(&path).expect("read");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].window_id, r1.window_id);
        assert_eq!(records[1].window_id, r2.window_id);

        let _ = std::fs::remove_file(&path);
    }
}
