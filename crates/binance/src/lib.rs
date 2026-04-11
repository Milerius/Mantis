//! Binance venue decoder for the Mantis SDK.
//!
//! Provides zero-allocation JSON decoders that convert Binance WebSocket
//! messages into [`mantis_events::HotEvent`] values.

#![deny(unsafe_code)]

mod decoder;
mod schema;
pub mod spawn;

pub use decoder::{BinanceDecoder, BinanceSymbolMapping, DecoderError, MAX_BINANCE_SYMBOLS};
pub use spawn::{spawn_binance_feed, FeedSpawnResult};
