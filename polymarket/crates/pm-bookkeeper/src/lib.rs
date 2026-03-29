//! Trade logging, P&L tracking, state persistence, and export.
//!
//! # Modules
//!
//! - [`trade_log`] — append-only JSONL writer/reader for [`pm_types::TradeRecord`] values.
//! - [`summary`] — compute aggregated performance statistics from a trade slice.
//! - [`export`] — CSV and JSON export utilities.
//! - [`recorder`] — live market data recorder for ticks and orderbook snapshots.

#![deny(unsafe_code)]

pub mod export;
pub mod recorder;
pub mod summary;
pub mod trade_log;

pub use export::{export_equity_curve, export_summary, export_trades_csv};
pub use recorder::{LiveRecorder, LiveSnapshot, SnapshotRecorder};
pub use summary::{TradeSummary, compute_summary};
pub use trade_log::{TradeLog, read_trade_log};
