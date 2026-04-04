//! Polymarket market data WebSocket adapter.
//!
//! Connects to `wss://ws-subscriptions-clob.polymarket.com/ws/market`,
//! subscribes to token IDs with `custom_feature_enabled: true`, and
//! forwards raw JSON text frames via the `FeedThread` callback.

use std::time::Duration;

use crate::feed::{BackoffConfig, FeedConfig, FeedHandle, FeedThread};
use crate::tuning::SocketTuning;
use crate::ws::WsConfig;

/// Polymarket CLOB market data WebSocket URL.
pub const WS_MARKET_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

/// Configuration for a Polymarket market data feed.
#[derive(Clone, Debug)]
pub struct PolymarketMarketConfig {
    /// Token IDs to subscribe to (Up + Down tokens for each market).
    pub token_ids: Vec<String>,
    /// CPU core to pin the feed thread to.
    pub core_id: Option<usize>,
    /// Reconnection backoff config.
    pub backoff: BackoffConfig,
}

impl PolymarketMarketConfig {
    /// Build the JSON subscription message.
    fn subscribe_msg(&self) -> String {
        // serde_json would be cleaner but this avoids a dep on the hot path
        let ids: Vec<String> = self
            .token_ids
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect();
        format!(
            r#"{{"assets_ids":[{}],"type":"market","custom_feature_enabled":true}}"#,
            ids.join(",")
        )
    }
}

/// Spawn a Polymarket market data feed thread.
///
/// The callback receives each raw JSON text frame (excluding `"PONG"`
/// responses). Event types include: `book`, `price_change`,
/// `last_trade_price`, `best_bid_ask`, `tick_size_change`,
/// `market_resolved`, `new_market`.
///
/// # Errors
///
/// Returns an error if the OS fails to spawn the thread.
pub fn spawn_market_feed<F>(
    config: PolymarketMarketConfig,
    on_message: F,
) -> Result<FeedHandle, std::io::Error>
where
    F: FnMut(&str) -> bool + Send + 'static,
{
    let feed_config = FeedConfig {
        name: "polymarket-market".to_owned(),
        ws: WsConfig {
            url: WS_MARKET_URL.to_owned(),
            subscribe_msg: Some(config.subscribe_msg()),
            ping_interval: Duration::from_secs(10),
            read_timeout: Some(Duration::from_secs(15)),
        },
        tuning: SocketTuning {
            core_id: config.core_id,
            #[cfg(feature = "tuning")]
            busy_poll_us: Some(50),
        },
        backoff: config.backoff,
    };

    FeedThread::spawn(feed_config, on_message)
}
