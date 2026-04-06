//! Venue-agnostic market-state engine for the Mantis SDK.
//!
//! Consumes `HotEvent` streams, maintains order books per instrument,
//! detects BBO price changes, and exposes derived metrics to inline
//! strategy callbacks. Zero allocation on the hot path.
//!
//! # Architecture
//!
//! - Passive state machine: `process(&mut self, event: &HotEvent)`
//! - Strategy runs on the same thread via callback with `&engine` access
//! - `ArrayBook<N>` for bounded venues (Polymarket, Binance depth20)
//! - No cross-thread sync — engine + strategy on one pinned core
//!
//! This crate is `no_std` by default.

#![no_std]
#![deny(unsafe_code)]

pub mod book;
mod engine;
mod state;

pub use book::{ArrayBook, OrderBook};
pub use engine::MarketStateEngine;
pub use state::{InstrumentState, TopOfBook, TradeInfo};
