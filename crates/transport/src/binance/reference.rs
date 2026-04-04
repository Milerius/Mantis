//! Binance reference price feed adapter.
//!
//! Connects to `wss://fstream.binance.com/ws/{symbol}@bookTicker` for
//! real-time BTC/USDT best bid/ask used as the fair-value anchor.
//! No subscription message needed — stream selection is in the URL path.

use crate::feed::{BackoffConfig, FeedConfig, FeedHandle, FeedThread};
use crate::tuning::SocketTuning;
use crate::ws::WsConfig;

/// Binance futures WebSocket base URL.
///
/// Uses `fstream.binance.com` (futures) which is accessible from more
/// regions than `stream.binance.com` (spot, geo-restricted in some locations).
const WS_BASE: &str = "wss://fstream.binance.com";

/// Build a Binance bookTicker stream URL for the given symbols.
///
/// Single symbol uses raw stream: `/ws/btcusdt@bookTicker`
/// Multiple symbols use combined stream: `/stream?streams=btcusdt@bookTicker/ethusdt@bookTicker`
fn book_ticker_url(symbols: &[&str]) -> String {
    let streams: Vec<String> = symbols.iter().map(|s| format!("{s}@bookTicker")).collect();
    if streams.len() == 1 {
        format!("{WS_BASE}/ws/{}", streams[0])
    } else {
        format!("{WS_BASE}/stream?streams={}", streams.join("/"))
    }
}

/// Configuration for a Binance reference price feed.
#[derive(Clone, Debug)]
pub struct BinanceReferenceConfig {
    /// Symbols to subscribe to (lowercase, e.g., `["btcusdt"]`).
    pub symbols: Vec<String>,
    /// CPU core to pin the feed thread to.
    pub core_id: Option<usize>,
    /// Reconnection backoff config.
    pub backoff: BackoffConfig,
}

impl Default for BinanceReferenceConfig {
    fn default() -> Self {
        Self {
            symbols: vec!["btcusdt".to_owned()],
            core_id: None,
            backoff: BackoffConfig::default(),
        }
    }
}

/// Spawn a Binance reference price feed thread.
///
/// The callback receives each raw JSON bookTicker message:
/// ```json
/// {"e":"bookTicker","s":"BTCUSDT","b":"66868.80","B":"13.109",
///  "a":"66868.90","A":"7.181","T":1775281508123,"E":1775281508123}
/// ```
///
/// # Errors
///
/// Returns an error if the OS fails to spawn the thread.
pub fn spawn_reference_feed<F>(
    config: BinanceReferenceConfig,
    on_message: F,
) -> Result<FeedHandle, std::io::Error>
where
    F: FnMut(&str) -> bool + Send + 'static,
{
    if config.symbols.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "at least one symbol is required",
        ));
    }

    let symbol_refs: Vec<&str> = config.symbols.iter().map(String::as_str).collect();
    let url = book_ticker_url(&symbol_refs);

    let feed_config = FeedConfig {
        name: "binance-reference".to_owned(),
        ws: WsConfig::binance(&url),
        tuning: SocketTuning {
            core_id: config.core_id,
            #[cfg(feature = "tuning")]
            busy_poll_us: None,
        },
        backoff: config.backoff,
    };

    FeedThread::spawn(feed_config, on_message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_ticker_url_single() {
        let url = book_ticker_url(&["btcusdt"]);
        assert_eq!(url, "wss://fstream.binance.com/ws/btcusdt@bookTicker");
    }

    #[test]
    fn book_ticker_url_multi() {
        let url = book_ticker_url(&["btcusdt", "ethusdt", "solusdt"]);
        assert_eq!(
            url,
            "wss://fstream.binance.com/stream?streams=btcusdt@bookTicker/ethusdt@bookTicker/solusdt@bookTicker"
        );
    }
}
