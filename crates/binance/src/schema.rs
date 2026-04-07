//! Zero-allocation Binance JSON schema types.

use serde::Deserialize;

/// Binance futures bookTicker message.
#[derive(Debug, Deserialize)]
#[expect(
    dead_code,
    reason = "fields present for JSON deserialization completeness"
)]
pub(crate) struct BinanceBookTicker<'a> {
    /// Event type (always `"bookTicker"`)
    pub e: &'a str,
    /// Symbol (e.g., `"BTCUSDT"`)
    pub s: &'a str,
    /// Best bid price
    pub b: &'a str,
    /// Best bid quantity
    #[serde(rename = "B")]
    pub bid_qty: &'a str,
    /// Best ask price
    pub a: &'a str,
    /// Best ask quantity
    #[serde(rename = "A")]
    pub ask_qty: &'a str,
    /// Trade time ms
    #[serde(rename = "T")]
    pub trade_time: u64,
    /// Event time ms
    #[serde(rename = "E")]
    pub event_time: u64,
}
