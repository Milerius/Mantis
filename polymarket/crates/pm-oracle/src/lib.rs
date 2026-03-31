//! Price data pipeline: historical download, replay, and live WebSocket feeds.

#![deny(unsafe_code)]

pub mod contract_model;
pub mod exchange_tracker;
pub mod downloader;
pub mod ema;
pub mod oracle_router;
pub mod pbt_downloader;
pub mod pbt_replay;
pub mod polybacktest;
pub mod polymarket;
pub mod price_buffer;
pub mod replay;
pub mod storage;
pub mod ws_binance;
pub mod ws_okx;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use contract_model::ContractPriceModel;
pub use exchange_tracker::ExchangePriceTracker;
pub use ema::EmaTracker;
pub use oracle_router::OracleRouter;
pub use pbt_replay::{PbtObservation, PbtReplay, pbt_to_price_observations};
pub use polybacktest::{PbtClient, PbtMarket, PbtSnapshot};
pub use price_buffer::PriceBuffer;
pub use replay::HistoricalReplay;
pub use ws_binance::BinanceWs;
pub use ws_okx::OkxWs;

pub use polymarket::{
    PolymarketTrade, asset_prefix, download_polymarket_day, download_polymarket_window,
    is_pm_cached, market_slug, market_slugs, pm_cache_path, read_polymarket_trades,
    timeframe_label, write_polymarket_trades,
};
