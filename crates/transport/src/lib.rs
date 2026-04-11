//! WebSocket transport and timing infrastructure for the Mantis SDK.
//!
//! This crate provides raw WebSocket feed threads (Polymarket, Binance)
//! and a timer thread for periodic events. Venue-specific JSON decoding
//! lives in `mantis-binance` and `mantis-polymarket` crates.
//!
//! # Architecture
//!
//! Each feed runs on a dedicated, CPU-pinned blocking thread. There is
//! no async runtime — the IO thread calls `tungstenite::WebSocket::read`
//! in a tight loop and pushes parsed events into an SPSC ring buffer.
//!
//! ```text
//! Polymarket Market WS ──> [SPSC 4096] ──┐
//! Polymarket User WS   ──> [SPSC 1024] ──┤──> market-state engine
//! Binance Reference WS ──> [SPSC 8192] ──┤
//! Timer                ──> [SPSC  256] ──┘
//! ```

#![deny(unsafe_code)]

pub mod binance;
mod feed;
mod monitor;
pub mod polymarket;
mod timer;
mod tuning;
mod ws;

pub use feed::{BackoffConfig, FeedConfig, FeedHandle, FeedThread};
pub use monitor::{FeedMonitor, MAX_FEEDS, MonitorFullError, StaleFeedInfo};
pub use timer::{TimerConfig, TimerConfigError, TimerSpawnError, TimerThread};
pub use tuning::SocketTuning;
pub use ws::{WsConfig, WsConnection, WsError};
