//! Polymarket market WebSocket decoder.

mod decoder;
pub(crate) mod schema;
pub mod spawn;

pub use decoder::PolymarketMarketDecoder;
pub use spawn::{FeedSpawnResult, spawn_polymarket_market_feed};
