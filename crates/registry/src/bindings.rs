//! Venue-specific bindings for instruments.

use mantis_types::Timestamp;

/// Stable Binance binding — symbol rarely changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinanceBinding {
    /// Binance symbol (e.g., "BTCUSDT").
    pub symbol: String,
}

/// Dynamic Polymarket binding — rotates every market window.
#[derive(Clone, Debug, Default)]
pub struct PolymarketBinding {
    /// Currently active window (being traded).
    pub current: Option<PolymarketWindowBinding>,
    /// Next upcoming window (pre-subscribed for seamless rollover).
    pub next: Option<PolymarketWindowBinding>,
}

/// One Polymarket market window (ephemeral — new every 5/15/60 minutes).
#[derive(Clone, Debug)]
pub struct PolymarketWindowBinding {
    /// Token ID for this outcome in this window (the WS subscription key).
    pub token_id: String,
    /// Market slug (e.g., "btc-updown-15m-1775280600").
    pub market_slug: String,
    /// Window open time.
    pub window_start: Timestamp,
    /// Window close time.
    pub window_end: Timestamp,
    /// Condition ID (shared between Up and Down tokens).
    pub condition_id: Option<String>,
}
