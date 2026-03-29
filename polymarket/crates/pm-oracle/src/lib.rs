//! Price data pipeline: historical download and replay for Binance and OKX.

#![deny(unsafe_code)]

pub mod contract_model;
pub mod downloader;
pub mod polymarket;
pub mod price_buffer;
pub mod replay;
pub mod storage;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use contract_model::ContractPriceModel;
pub use price_buffer::PriceBuffer;
pub use replay::HistoricalReplay;

pub use polymarket::{
    PolymarketTrade, asset_prefix, download_polymarket_day, download_polymarket_window,
    is_pm_cached, market_slug, market_slugs, pm_cache_path, read_polymarket_trades,
    timeframe_label, write_polymarket_trades,
};
