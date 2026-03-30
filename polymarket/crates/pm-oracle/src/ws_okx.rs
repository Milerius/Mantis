//! OKX WebSocket trade feed.
//!
//! Connects to `wss://ws.okx.com:8443/ws/v5/public`, subscribes to the
//! `trades` channel for each configured asset, and publishes [`Tick`]s to
//! a broadcast channel. Reconnects automatically on disconnect.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pm_types::{Asset, ExchangeSource, Price, Tick};
use serde::Deserialize;
use tokio::sync::broadcast;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

const OKX_WS_URL: &str = "wss://ws.okx.com:8443/ws/v5/public";

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors from the OKX WebSocket client.
#[derive(Debug, thiserror::Error)]
pub enum OkxWsError {
    /// WebSocket connection or I/O error.
    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    /// JSON serialisation / deserialisation error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, OkxWsError>;

// ─── Wire types ──────────────────────────────────────────────────────────────

/// A single trade entry inside an OKX `trades` push event.
#[derive(Debug, Deserialize)]
struct OkxTradeEntry {
    /// Instrument ID, e.g. `"BTC-USDT-SWAP"`.
    #[serde(rename = "instId")]
    inst_id: String,
    /// Price as a decimal string.
    #[serde(rename = "px")]
    px: String,
    /// Timestamp in milliseconds (as a string in the OKX API).
    #[serde(rename = "ts")]
    ts: String,
}

/// Top-level OKX push message containing an array of trade entries.
#[derive(Debug, Deserialize)]
struct OkxPushMessage {
    /// Trade data array.
    #[serde(default)]
    data: Vec<OkxTradeEntry>,
}

// ─── Parsing helpers ─────────────────────────────────────────────────────────

/// Map an OKX `instId` to an [`Asset`], returning `None` for unknowns.
fn inst_id_to_asset(inst_id: &str) -> Option<Asset> {
    Asset::ALL
        .into_iter()
        .find(|a| a.okx_inst_id() == inst_id)
}

/// Parse a raw OKX push JSON message into zero or more [`Tick`]s.
///
/// OKX batches multiple trades in a single push; this function returns one
/// `Tick` per entry. Entries with unknown symbols or invalid prices are
/// silently skipped.
pub(crate) fn parse_push_message(raw: &str) -> Vec<Tick> {
    let msg: OkxPushMessage = match serde_json::from_str(raw) {
        Ok(m) => m,
        Err(_) => return vec![],
    };

    msg.data
        .into_iter()
        .filter_map(|entry| {
            let asset = inst_id_to_asset(&entry.inst_id)?;
            let price_f64: f64 = entry.px.parse().ok()?;
            let price = Price::new(price_f64)?;
            let timestamp_ms: u64 = entry.ts.parse().ok()?;
            Some(Tick {
                asset,
                price,
                timestamp_ms,
                source: ExchangeSource::Okx,
            })
        })
        .collect()
}

/// Build the JSON subscribe payload for the given assets.
fn subscribe_message(assets: &[Asset]) -> serde_json::Value {
    let args: Vec<serde_json::Value> = assets
        .iter()
        .map(|a| {
            serde_json::json!({
                "channel": "trades",
                "instId": a.okx_inst_id()
            })
        })
        .collect();
    serde_json::json!({
        "op": "subscribe",
        "args": args
    })
}

// ─── OkxWs ───────────────────────────────────────────────────────────────────

/// OKX WebSocket feed for a set of assets.
///
/// Subscribes to the `trades` channel on the OKX public WebSocket and
/// forwards [`Tick`]s to a broadcast channel.
pub struct OkxWs {
    assets: Vec<Asset>,
}

impl OkxWs {
    /// Create a new [`OkxWs`] that will subscribe to the given assets.
    #[must_use]
    pub fn new(assets: Vec<Asset>) -> Self {
        Self { assets }
    }

    /// Connect to OKX and stream ticks until `shutdown` is cancelled.
    ///
    /// After connecting, sends the subscribe message for all configured assets.
    /// Reconnects automatically after a 1-second delay when the connection
    /// drops. The function returns only once `shutdown` is cancelled.
    ///
    /// # Errors
    ///
    /// Returns an error if a fatal protocol or transport error occurs that the
    /// reconnect loop cannot recover from.
    pub async fn run(
        self,
        tick_tx: broadcast::Sender<Tick>,
        shutdown: CancellationToken,
    ) -> Result<()> {
        let sub_msg =
            Message::Text(subscribe_message(&self.assets).to_string().into());

        loop {
            if shutdown.is_cancelled() {
                break;
            }

            match connect_async(OKX_WS_URL).await {
                Ok((ws_stream, _)) => {
                    debug!("OKX WS connected: {OKX_WS_URL}");
                    let (mut write, mut read) = ws_stream.split();

                    // Send the subscribe payload.
                    if let Err(e) = write.send(sub_msg.clone()).await {
                        warn!("OKX WS subscribe error: {e}");
                        continue;
                    }

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
                                        for tick in parse_push_message(&text) {
                                            let _ = tick_tx.send(tick);
                                        }
                                    }
                                    Some(Err(e)) => {
                                        warn!("OKX WS error: {e}");
                                        break;
                                    }
                                    None => {
                                        warn!("OKX WS stream closed");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("OKX WS connect error: {e}");
                }
            }

            if shutdown.is_cancelled() {
                break;
            }
            warn!("OKX WS: reconnecting in 1s");
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
    fn parse_btc_push_message() {
        let raw = r#"{"data":[{"instId":"BTC-USDT-SWAP","px":"84230.50","ts":"1774782000123"}]}"#;
        let ticks = parse_push_message(raw);
        assert_eq!(ticks.len(), 1);
        let tick = &ticks[0];
        assert_eq!(tick.asset, Asset::Btc);
        assert!((tick.price.as_f64() - 84_230.50).abs() < 1e-9);
        assert_eq!(tick.timestamp_ms, 1_774_782_000_123);
        assert_eq!(tick.source, ExchangeSource::Okx);
    }

    #[test]
    fn parse_multi_entry_push() {
        let raw = r#"{
            "data": [
                {"instId":"BTC-USDT-SWAP","px":"84000.00","ts":"1774782000000"},
                {"instId":"ETH-USDT-SWAP","px":"3200.00","ts":"1774782000001"}
            ]
        }"#;
        let ticks = parse_push_message(raw);
        assert_eq!(ticks.len(), 2);
        assert_eq!(ticks[0].asset, Asset::Btc);
        assert_eq!(ticks[1].asset, Asset::Eth);
    }

    #[test]
    fn parse_sol_push_message() {
        let raw = r#"{"data":[{"instId":"SOL-USDT-SWAP","px":"150.00","ts":"1774782001000"}]}"#;
        let ticks = parse_push_message(raw);
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].asset, Asset::Sol);
    }

    #[test]
    fn parse_xrp_push_message() {
        let raw = r#"{"data":[{"instId":"XRP-USDT-SWAP","px":"0.60","ts":"1774782002000"}]}"#;
        let ticks = parse_push_message(raw);
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].asset, Asset::Xrp);
    }

    #[test]
    fn parse_unknown_inst_id_skipped() {
        let raw = r#"{"data":[{"instId":"DOGE-USDT-SWAP","px":"0.10","ts":"1774782003000"}]}"#;
        let ticks = parse_push_message(raw);
        assert!(ticks.is_empty());
    }

    #[test]
    fn parse_invalid_price_skipped() {
        let raw =
            r#"{"data":[{"instId":"BTC-USDT-SWAP","px":"not_a_number","ts":"1774782004000"}]}"#;
        let ticks = parse_push_message(raw);
        assert!(ticks.is_empty());
    }

    #[test]
    fn parse_empty_data_returns_empty() {
        let raw = r#"{"data":[]}"#;
        let ticks = parse_push_message(raw);
        assert!(ticks.is_empty());
    }

    #[test]
    fn parse_subscribe_ack_returns_empty() {
        // OKX sends subscription acks without a `data` key.
        let raw = r#"{"event":"subscribe","arg":{"channel":"trades","instId":"BTC-USDT-SWAP"}}"#;
        let ticks = parse_push_message(raw);
        assert!(ticks.is_empty());
    }

    #[test]
    fn subscribe_message_contains_all_assets() {
        let assets = vec![Asset::Btc, Asset::Eth];
        let msg = subscribe_message(&assets);
        let args = msg["args"].as_array().expect("args must be array");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0]["instId"], "BTC-USDT-SWAP");
        assert_eq!(args[1]["instId"], "ETH-USDT-SWAP");
        assert_eq!(msg["op"], "subscribe");
    }
}
