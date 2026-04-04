//! WebSocket transport ingest layer for the Mantis SDK.
//!
//! This crate connects to venue WebSocket feeds (Polymarket, Binance),
//! parses JSON into [`mantis_events::HotEvent`] values, and pushes them
//! into [`mantis_queue::SpscRingCopy`] queues for consumption by
//! downstream engine threads.
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
pub mod polymarket;
mod tuning;
mod ws;

pub use feed::{BackoffConfig, FeedConfig, FeedHandle, FeedThread};
pub use tuning::SocketTuning;
pub use ws::{WsConfig, WsConnection, WsError};
