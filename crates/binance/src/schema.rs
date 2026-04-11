//! Zero-allocation Binance JSON schema types.

use serde::Deserialize;

/// Binance futures bookTicker message.
///
/// Only fields used by the decoder are deserialized. `T` (trade time) and
/// `E` (event time) are intentionally omitted -- serde skips unknown fields
/// by default, saving ~3.6% parse time per message.
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
}

/// Binance combined stream wrapper for multi-symbol subscriptions.
#[derive(Debug, Deserialize)]
pub(crate) struct BinanceCombinedStream<'a> {
    #[expect(dead_code, reason = "present for JSON completeness")]
    pub stream: &'a str,
    pub data: BinanceBookTicker<'a>,
}
