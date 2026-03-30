//! Binance WebSocket trade feed.
//!
//! Streams real-time trade events from `wss://stream.binance.com:9443/ws/{symbol}@trade`
//! for each configured asset and publishes [`Tick`]s to a broadcast channel.
//! Reconnects automatically on disconnect.

use std::time::Duration;

use futures_util::StreamExt;
use pm_types::{Asset, ExchangeSource, Price, Tick};
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors from the Binance WebSocket client.
#[derive(Debug, thiserror::Error)]
pub enum BinanceWsError {
    /// WebSocket connection or I/O error.
    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    /// JSON deserialisation failure.
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// A price string could not be parsed as `f64`.
    #[error("invalid price string: {0}")]
    InvalidPrice(String),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, BinanceWsError>;

// ─── Wire types ──────────────────────────────────────────────────────────────

/// Binance trade event (`@trade` stream).
#[derive(Debug, Deserialize)]
struct TradeEvent {
    /// Event type — always `"trade"`.
    #[serde(rename = "e")]
    event_type: String,
    /// Symbol, e.g. `"BTCUSDT"`.
    #[serde(rename = "s")]
    symbol: String,
    /// Price as a decimal string.
    #[serde(rename = "p")]
    price: String,
    /// Trade time in milliseconds since Unix epoch.
    #[serde(rename = "T")]
    timestamp_ms: u64,
}

// ─── Parsing helpers ─────────────────────────────────────────────────────────

/// Map a Binance symbol string to an [`Asset`], returning `None` for unknowns.
fn symbol_to_asset(symbol: &str) -> Option<Asset> {
    Asset::ALL
        .into_iter()
        .find(|a| a.binance_symbol() == symbol)
}

/// Parse a raw Binance `@trade` JSON message into a [`Tick`].
///
/// Returns `None` if the event type is not `"trade"`, the symbol is unknown,
/// or the price is invalid.
pub(crate) fn parse_trade_message(raw: &str) -> Option<Tick> {
    let event: TradeEvent = serde_json::from_str(raw).ok()?;
    if event.event_type != "trade" {
        return None;
    }
    let asset = symbol_to_asset(&event.symbol)?;
    let price_f64: f64 = event.price.parse().ok()?;
    let price = Price::new(price_f64)?;
    Some(Tick {
        asset,
        price,
        timestamp_ms: event.timestamp_ms,
        source: ExchangeSource::Binance,
    })
}

// ─── BinanceWs ───────────────────────────────────────────────────────────────

/// Binance WebSocket feed for a set of assets.
///
/// Connects to the combined trade stream for each asset and forwards
/// [`Tick`]s to a broadcast channel.
pub struct BinanceWs {
    assets: Vec<Asset>,
}

impl BinanceWs {
    /// Create a new [`BinanceWs`] that will subscribe to the given assets.
    #[must_use]
    pub fn new(assets: Vec<Asset>) -> Self {
        Self { assets }
    }

    /// Connect to Binance and stream ticks until `shutdown` is cancelled.
    ///
    /// Reconnects automatically after a 1-second delay when the connection
    /// drops. The function returns only once `shutdown` is cancelled.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial connection attempt fails and cannot be
    /// recovered. In practice the loop retries indefinitely, so errors are rare.
    pub async fn run(
        self,
        tick_tx: broadcast::Sender<Tick>,
        shutdown: CancellationToken,
    ) -> Result<()> {
        // Build the combined stream path: /ws/btcusdt@trade/ethusdt@trade/…
        let streams: Vec<String> = self
            .assets
            .iter()
            .map(|a| format!("{}@trade", a.binance_symbol().to_lowercase()))
            .collect();
        let path = streams.join("/");
        let url = format!("wss://stream.binance.com:9443/ws/{path}");

        loop {
            if shutdown.is_cancelled() {
                break;
            }

            match connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    debug!("Binance WS connected: {url}");
                    let (_, mut read) = ws_stream.split();

                    loop {
                        tokio::select! {
                            () = shutdown.cancelled() => return Ok(()),
                            msg = read.next() => {
                                match msg {
                                    Some(Ok(m)) => {
                                        let text = match m.to_text() {
                                            Ok(t) => t.to_owned(),
                                            Err(_) => continue,
                                        };
                                        if let Some(tick) = parse_trade_message(&text) {
                                            // Ignore send errors — no receivers is fine.
                                            let _ = tick_tx.send(tick);
                                        }
                                    }
                                    Some(Err(e)) => {
                                        warn!("Binance WS error: {e}");
                                        break;
                                    }
                                    None => {
                                        warn!("Binance WS stream closed");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Binance WS connect error: {e}");
                }
            }

            if shutdown.is_cancelled() {
                break;
            }
            warn!("Binance WS: reconnecting in 1s");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;

    #[test]
    fn parse_btc_trade_message() {
        let raw = r#"{"e":"trade","s":"BTCUSDT","p":"84230.50","T":1774782000123}"#;
        let tick = parse_trade_message(raw).expect("should parse BTC trade");
        assert_eq!(tick.asset, Asset::Btc);
        assert!((tick.price.as_f64() - 84_230.50).abs() < 1e-9);
        assert_eq!(tick.timestamp_ms, 1_774_782_000_123);
        assert_eq!(tick.source, ExchangeSource::Binance);
    }

    #[test]
    fn parse_eth_trade_message() {
        let raw = r#"{"e":"trade","s":"ETHUSDT","p":"3200.00","T":1774782001000}"#;
        let tick = parse_trade_message(raw).expect("should parse ETH trade");
        assert_eq!(tick.asset, Asset::Eth);
        assert!((tick.price.as_f64() - 3_200.0).abs() < 1e-9);
        assert_eq!(tick.source, ExchangeSource::Binance);
    }

    #[test]
    fn parse_sol_trade_message() {
        let raw = r#"{"e":"trade","s":"SOLUSDT","p":"150.25","T":1774782002000}"#;
        let tick = parse_trade_message(raw).expect("should parse SOL trade");
        assert_eq!(tick.asset, Asset::Sol);
    }

    #[test]
    fn parse_xrp_trade_message() {
        let raw = r#"{"e":"trade","s":"XRPUSDT","p":"0.55","T":1774782003000}"#;
        let tick = parse_trade_message(raw).expect("should parse XRP trade");
        assert_eq!(tick.asset, Asset::Xrp);
    }

    #[test]
    fn parse_unknown_symbol_returns_none() {
        let raw = r#"{"e":"trade","s":"DOGEUSDT","p":"0.10","T":1774782004000}"#;
        assert!(parse_trade_message(raw).is_none());
    }

    #[test]
    fn parse_non_trade_event_returns_none() {
        let raw = r#"{"e":"kline","s":"BTCUSDT","p":"84000.00","T":1774782005000}"#;
        assert!(parse_trade_message(raw).is_none());
    }

    #[test]
    fn parse_invalid_price_returns_none() {
        let raw = r#"{"e":"trade","s":"BTCUSDT","p":"not_a_number","T":1774782006000}"#;
        assert!(parse_trade_message(raw).is_none());
    }

    #[test]
    fn parse_negative_price_returns_none() {
        let raw = r#"{"e":"trade","s":"BTCUSDT","p":"-100.00","T":1774782007000}"#;
        assert!(parse_trade_message(raw).is_none());
    }
}
