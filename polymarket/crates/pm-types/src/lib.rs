//! Domain types for the Polymarket trading bot.
//!
//! `no_std` by default. Enable the `std` feature for serde serialization /
//! deserialization support and the [`config`] module (which requires heap
//! allocation for TOML parsing).

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

pub mod asset;
#[cfg(feature = "std")]
pub mod config;
pub mod market;
pub mod price;
pub mod trade;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use asset::{Asset, ExchangeSource, Side, Timeframe};
#[cfg(feature = "std")]
pub use config::{AssetConfig, BacktestConfig, BotConfig, BotSection, DataConfig, Mode};
pub use market::{OrderId, Signal, Tick, Window, WindowId};
pub use price::{ContractPrice, Edge, Pnl, Price};
pub use trade::{Fill, OpenPosition, OrderReason, Rejection, SizedOrder, TradeRecord};
