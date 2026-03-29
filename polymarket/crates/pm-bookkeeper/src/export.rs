//! CSV and JSON export utilities for trade data and summary statistics.
//!
//! Three export formats are provided:
//! - [`export_equity_curve`] — CSV equity curve (trade number, cumulative P&L, timestamp).
//! - [`export_summary`] — pretty-printed JSON [`TradeSummary`].
//! - [`export_trades_csv`] — full CSV with every [`TradeRecord`] field.

use std::{
    fs::{File, create_dir_all},
    io::{self, BufWriter, Write},
    path::Path,
};

use pm_types::TradeRecord;

use crate::summary::TradeSummary;

// ─── export_equity_curve ─────────────────────────────────────────────────────

/// Write an equity curve CSV to `path`.
///
/// Columns: `trade_number,cumulative_pnl,timestamp_ms`
///
/// `timestamp_ms` is taken from the trade's `closed_at_ms` field.
///
/// # Errors
///
/// Returns [`io::Error`] if the file cannot be created or written.
pub fn export_equity_curve(path: &Path, trades: &[TradeRecord]) -> io::Result<()> {
    let mut writer = create_csv_writer(path)?;
    writer.write_all(b"trade_number,cumulative_pnl,timestamp_ms\n")?;
    let mut cumulative = 0.0_f64;
    for (i, trade) in trades.iter().enumerate() {
        cumulative += trade.pnl.as_f64();
        writeln!(writer, "{},{cumulative},{}", i + 1, trade.closed_at_ms)?;
    }
    writer.flush()
}

// ─── export_summary ──────────────────────────────────────────────────────────

/// Write a pretty-printed JSON representation of `summary` to `path`.
///
/// # Errors
///
/// Returns [`io::Error`] if the file cannot be created, the summary cannot be
/// serialised, or the write fails.
pub fn export_summary(path: &Path, summary: &TradeSummary) -> io::Result<()> {
    let json = serde_json::to_string_pretty(summary)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut writer = create_csv_writer(path)?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

// ─── export_trades_csv ───────────────────────────────────────────────────────

/// Write all [`TradeRecord`] fields to a CSV file at `path`.
///
/// Columns:
/// `window_id,asset,side,entry_price,exit_price,size_usdc,pnl,opened_at_ms,closed_at_ms,close_reason`
///
/// # Errors
///
/// Returns [`io::Error`] if the file cannot be created or written.
pub fn export_trades_csv(path: &Path, trades: &[TradeRecord]) -> io::Result<()> {
    let mut writer = create_csv_writer(path)?;
    writer.write_all(
        b"window_id,asset,side,entry_price,exit_price,size_usdc,pnl,opened_at_ms,closed_at_ms,close_reason\n",
    )?;
    for trade in trades {
        writeln!(
            writer,
            "{},{},{},{},{},{},{},{},{},{}",
            trade.window_id,
            trade.asset,
            trade.side,
            trade.entry_price,
            trade.exit_price,
            trade.size_usdc,
            trade.pnl,
            trade.opened_at_ms,
            trade.closed_at_ms,
            trade.close_reason,
        )?;
    }
    writer.flush()
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Create parent directories and open a buffered writer for `path`.
fn create_csv_writer(path: &Path) -> io::Result<BufWriter<File>> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    Ok(BufWriter::new(file))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::fs;

    use pm_types::{Asset, ContractPrice, OrderReason, Pnl, Side, StrategyId, WindowId};

    use super::*;
    use crate::summary::compute_summary;

    fn make_record(window: u64, pnl: f64) -> TradeRecord {
        TradeRecord {
            window_id: WindowId::new(window),
            asset: Asset::Btc,
            side: Side::Up,
            entry_price: ContractPrice::new(0.45).expect("valid entry"),
            exit_price: ContractPrice::new(0.80).expect("valid exit"),
            size_usdc: 25.0,
            pnl: Pnl::new(pnl).expect("finite pnl"),
            opened_at_ms: 1_000,
            closed_at_ms: 3_600_000,
            close_reason: OrderReason::ExpiryClose,
            strategy_id: StrategyId::EarlyDirectional,
        }
    }

    #[test]
    fn equity_curve_creates_valid_csv() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_export_test_equity");
        let path = dir.join("equity.csv");
        let _ = fs::remove_file(&path);

        let trades = vec![
            make_record(1, 10.0),
            make_record(2, -5.0),
            make_record(3, 8.0),
        ];
        export_equity_curve(&path, &trades).expect("export equity curve");

        let content = fs::read_to_string(&path).expect("read csv");
        let lines: Vec<&str> = content.lines().collect();

        // Header + 3 data rows
        assert_eq!(
            lines.len(),
            4,
            "expected header + 3 data lines, got:\n{content}"
        );
        assert_eq!(lines[0], "trade_number,cumulative_pnl,timestamp_ms");
        assert!(lines[1].starts_with("1,10,"), "line1={}", lines[1]);
        assert!(lines[2].starts_with("2,5,"), "line2={}", lines[2]);
        assert!(lines[3].starts_with("3,13,"), "line3={}", lines[3]);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn equity_curve_empty_trades() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_export_test_equity_empty");
        let path = dir.join("equity.csv");
        let _ = fs::remove_file(&path);

        export_equity_curve(&path, &[]).expect("export empty");

        let content = fs::read_to_string(&path).expect("read csv");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "only header when no trades");
        assert_eq!(lines[0], "trade_number,cumulative_pnl,timestamp_ms");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn summary_creates_valid_json() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_export_test_summary");
        let path = dir.join("summary.json");
        let _ = fs::remove_file(&path);

        let trades = vec![make_record(1, 10.0), make_record(2, -5.0)];
        let summary = compute_summary(&trades);
        export_summary(&path, &summary).expect("export summary");

        let content = fs::read_to_string(&path).expect("read json");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");

        assert_eq!(parsed["total_trades"], 2);
        assert_eq!(parsed["wins"], 1);
        assert_eq!(parsed["losses"], 1);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn trades_csv_creates_valid_csv() {
        let dir = std::env::temp_dir().join("pm_bookkeeper_export_test_trades_csv");
        let path = dir.join("trades.csv");
        let _ = fs::remove_file(&path);

        let trades = vec![make_record(1, 10.0), make_record(2, -5.0)];
        export_trades_csv(&path, &trades).expect("export trades csv");

        let content = fs::read_to_string(&path).expect("read csv");
        let lines: Vec<&str> = content.lines().collect();

        // Header + 2 data rows
        assert_eq!(lines.len(), 3, "expected header + 2 data lines");
        assert!(
            lines[0].starts_with("window_id,asset,side,entry_price"),
            "header mismatch: {}",
            lines[0]
        );
        // First data row should contain W1 and BTC
        assert!(lines[1].contains("W1"), "row1 missing W1: {}", lines[1]);
        assert!(lines[1].contains("BTC"), "row1 missing BTC: {}", lines[1]);
        assert!(lines[2].contains("W2"), "row2 missing W2: {}", lines[2]);

        let _ = fs::remove_file(&path);
    }
}
