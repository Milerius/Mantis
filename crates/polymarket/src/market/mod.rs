//! Polymarket market WebSocket decoder.

mod decoder;
pub(crate) mod schema;
pub mod spawn;

pub use decoder::PolymarketMarketDecoder;
