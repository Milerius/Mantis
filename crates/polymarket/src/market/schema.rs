//! Zero-allocation serde structs for Polymarket WebSocket messages.

use serde::Deserialize;

/// Polymarket `"book"` message — orderbook snapshot with variable-length levels.
#[derive(Debug, Deserialize)]
#[expect(
    dead_code,
    reason = "fields present for JSON deserialization completeness"
)]
pub(crate) struct PolymarketBookMsg<'a> {
    /// The Polymarket token identifier for this asset.
    pub asset_id: &'a str,
    /// Bid-side levels.
    #[serde(default)]
    pub bids: Vec<BookLevel<'a>>,
    /// Ask-side levels.
    #[serde(default)]
    pub asks: Vec<BookLevel<'a>>,
}

/// A single orderbook level.
#[derive(Debug, Deserialize)]
#[expect(
    dead_code,
    reason = "fields present for JSON deserialization completeness"
)]
pub(crate) struct BookLevel<'a> {
    /// Price as a decimal string.
    pub price: &'a str,
    /// Size as a decimal string.
    pub size: &'a str,
}

/// Polymarket `"price_change"` message.
#[derive(Debug, Deserialize)]
#[expect(
    dead_code,
    reason = "fields present for JSON deserialization completeness"
)]
pub(crate) struct PolymarketPriceChangeMsg<'a> {
    /// The Polymarket token identifier for this asset.
    pub asset_id: &'a str,
    /// Price as a decimal string.
    pub price: &'a str,
    /// Size as a decimal string.
    pub size: &'a str,
    /// Side string (`"BUY"` or `"SELL"`).
    pub side: &'a str,
}

/// Polymarket `"last_trade_price"` message.
#[derive(Debug, Deserialize)]
#[expect(
    dead_code,
    reason = "fields present for JSON deserialization completeness"
)]
pub(crate) struct PolymarketTradeMsg<'a> {
    /// The Polymarket token identifier for this asset.
    pub asset_id: &'a str,
    /// Price as a decimal string.
    pub price: &'a str,
    /// Size as a decimal string.
    pub size: &'a str,
    /// Optional side string (`"BUY"` or `"SELL"`).
    #[serde(default)]
    pub side: Option<&'a str>,
}
