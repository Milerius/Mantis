//! Polymarket market discovery and live orderbook tracking.
//!
//! This crate provides four modules:
//!
//! - [`scanner`] — polls the Gamma REST API to discover active crypto Up/Down
//!   markets
//! - [`orderbook`] — maintains best-bid/ask snapshots via the Polymarket CLOB
//!   WebSocket
//! - [`manager`] — combines scanner results and orderbook state into a single
//!   coherent view of the live market
//! - [`ws_polymarket`] — live WebSocket client that feeds real-time
//!   best-bid/ask prices into the shared [`OrderbookTracker`]
//!
//! # Typical usage
//!
//! ```rust,no_run
//! use std::time::Duration;
//! use pm_market::manager::MarketManager;
//! use pm_market::scanner::scan_active_markets;
//! use pm_types::Asset;
//! use reqwest::Client;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new();
//! let mut mgr = MarketManager::new(Duration::from_secs(60));
//!
//! let markets = scan_active_markets(&client, &Asset::ALL).await?;
//! mgr.update_markets(markets);
//!
//! for market in mgr.active_markets() {
//!     println!("{} {} {}", market.asset, market.timeframe, market.condition_id);
//! }
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]

pub mod manager;
pub mod orderbook;
pub mod scanner;
pub mod ws_polymarket;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use manager::MarketManager;
pub use orderbook::{OrderbookSnapshot, OrderbookTracker};
pub use scanner::{MarketInfo, ScanError, scan_active_markets};
pub use ws_polymarket::{NewTokensSender, PolymarketWs};
