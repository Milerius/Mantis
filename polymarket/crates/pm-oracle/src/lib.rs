//! Price data pipeline: historical download and replay for Binance and OKX.

#![deny(unsafe_code)]

pub mod downloader;
pub mod price_buffer;
pub mod replay;
pub mod storage;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use price_buffer::PriceBuffer;
pub use replay::HistoricalReplay;
