//! Polymarket CLOB WebSocket client for real-time best-bid/ask updates.
//!
//! Connects to `wss://ws-subscriptions-clob.polymarket.com/ws/market`, subscribes
//! to a set of token IDs, and updates a shared [`OrderbookTracker`] on every
//! `best_bid_ask` event received from the server.
//!
//! # Heartbeat
//!
//! The Polymarket WebSocket server **requires** a `PING` text frame every 10
//! seconds or it will disconnect the client. This module sends the heartbeat via
//! a [`tokio::time::interval`] on the write half of the socket.
//!
//! # Dynamic subscriptions
//!
//! New token IDs discovered by the market scanner can be fed in via a
//! [`tokio::sync::mpsc`] channel. The running task will send an updated
//! subscribe message whenever new IDs arrive.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pm_types::{Asset, Timeframe};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::orderbook::{LatestPrices, OrderbookTracker, WS_CLOB_URL};

// ─── Market resolution ──────────────────────────────────────────────────────

/// Notification emitted when the Polymarket WebSocket sends a `market_resolved`
/// event indicating that a market's Chainlink oracle has settled.
#[derive(Debug, Clone)]
pub struct MarketResolution {
    /// The condition ID of the resolved market.
    pub condition_id: String,
    /// The token ID of the winning outcome (Up or Down).
    pub winning_token_id: String,
    /// Unix timestamp in milliseconds when the resolution was observed.
    pub timestamp_ms: u64,
}

// ─── Wire types ──────────────────────────────────────────────────────────────

/// The `best_bid_ask` event received from the Polymarket market channel.
///
/// The server sends this whenever the best bid or ask price changes for a
/// token. `best_bid` and `best_ask` are string-encoded decimals in `[0, 1]`.
#[derive(Debug, Deserialize)]
pub(crate) struct BestBidAskEvent {
    /// Token ID that this update is for.
    asset_id: String,
    /// Condition ID of the market this token belongs to.
    market: String,
    /// Best bid price (string decimal, e.g. `"0.48"`).
    best_bid: String,
    /// Best ask price (string decimal, e.g. `"0.52"`).
    best_ask: String,
    /// Unix timestamp in seconds (string decimal).
    #[serde(default)]
    timestamp: String,
}

/// Envelope wrapper — the Polymarket WS sends events with an `event_type` field
/// at the top level so we can dispatch without deserialising the full payload.
#[derive(Debug, Deserialize)]
struct WsEnvelope {
    /// Discriminator, e.g. `"best_bid_ask"`, `"book"`, `"price_change"`, …
    event_type: String,
    /// Token ID (present on most event types).
    #[serde(default)]
    asset_id: String,
    /// Condition ID (present on most event types).
    #[serde(default)]
    market: String,
    /// Best bid (only on `best_bid_ask`).
    #[serde(default)]
    best_bid: String,
    /// Best ask (only on `best_bid_ask`).
    #[serde(default)]
    best_ask: String,
    /// Unix timestamp in seconds (string).
    #[serde(default)]
    timestamp: String,
}

// ─── Parsing ─────────────────────────────────────────────────────────────────

/// Parse a raw WebSocket text message, returning a [`BestBidAskEvent`] when
/// the message is a `best_bid_ask` event with valid bid/ask prices.
///
/// Returns `None` for non-`best_bid_ask` events or parse failures.
pub(crate) fn parse_best_bid_ask(raw: &str) -> Option<BestBidAskEvent> {
    // Fast path: skip messages that obviously aren't best_bid_ask.
    if !raw.contains("best_bid_ask") {
        return None;
    }
    let env: WsEnvelope = serde_json::from_str(raw).ok()?;
    if env.event_type != "best_bid_ask" {
        return None;
    }
    Some(BestBidAskEvent {
        asset_id: env.asset_id,
        market: env.market,
        best_bid: env.best_bid,
        best_ask: env.best_ask,
        timestamp: env.timestamp,
    })
}

/// Parse a raw WebSocket text message, returning a [`MarketResolution`] when
/// the message is a `market_resolved` event.
///
/// Returns `None` for non-`market_resolved` events or parse failures.
pub(crate) fn parse_market_resolved(raw: &str) -> Option<MarketResolution> {
    if !raw.contains("market_resolved") {
        return None;
    }
    let env: WsEnvelope = serde_json::from_str(raw).ok()?;
    if env.event_type != "market_resolved" {
        return None;
    }
    if env.asset_id.is_empty() || env.market.is_empty() {
        return None;
    }
    let timestamp_ms: u64 = env
        .timestamp
        .parse::<f64>()
        .map_or_else(
            |_| {
                #[expect(clippy::cast_possible_truncation, reason = "millis since epoch fits in u64 for centuries")]
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                ms
            },
            |s| {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "timestamp_ms fits in u64")]
                let ms = (s * 1_000.0) as u64;
                ms
            },
        );
    Some(MarketResolution {
        condition_id: env.market,
        winning_token_id: env.asset_id,
        timestamp_ms,
    })
}

// ─── Subscription message builder ────────────────────────────────────────────

/// Build the JSON subscribe message for the given token IDs.
fn build_subscribe_message(token_ids: &[String]) -> String {
    let ids_json: Vec<serde_json::Value> =
        token_ids.iter().map(|id| serde_json::Value::String(id.clone())).collect();
    serde_json::json!({
        "assets_ids": ids_json,
        "type": "market",
        "custom_feature_enabled": true
    })
    .to_string()
}

// ─── PolymarketWs ─────────────────────────────────────────────────────────────

/// Shared container for market resolution events detected by the WebSocket.
///
/// The paper loop drains this periodically to resolve positions.
pub type SharedResolutions = Arc<Mutex<Vec<MarketResolution>>>;

/// Shared map: `token_id` -> `(Asset, Timeframe, is_up)`.
///
/// Populated by the scanner (via paper loop), read by the WS handler to map
/// incoming `best_bid_ask` events to the correct (Asset, Timeframe) slot in
/// [`LatestPrices`].
pub type SharedTokenAssetMap = Arc<Mutex<HashMap<String, (Asset, Timeframe, bool)>>>;

/// Shared latest-prices cache, indexed by `(Asset, Timeframe)`.
pub type SharedLatestPrices = Arc<Mutex<LatestPrices>>;

/// Polymarket CLOB WebSocket client.
///
/// Connects to the market channel, subscribes to the initial set of token IDs,
/// and keeps the shared [`OrderbookTracker`] up to date. Handles PING/PONG
/// heartbeat and auto-reconnects on disconnect.
pub struct PolymarketWs {
    /// Initial token IDs to subscribe to on connect.
    token_ids: Vec<String>,
    /// Channel receiver for dynamically adding new token IDs at runtime.
    new_tokens_rx: mpsc::Receiver<Vec<String>>,
    /// Shared resolution events pushed when `market_resolved` is received.
    resolved_markets: SharedResolutions,
    /// Reverse lookup: token_id -> (Asset, Timeframe, is_up).
    token_asset_map: SharedTokenAssetMap,
    /// Shared latest prices cache.
    latest_prices: SharedLatestPrices,
}

/// Sender half returned to the caller for pushing new token IDs.
pub type NewTokensSender = mpsc::Sender<Vec<String>>;

impl PolymarketWs {
    /// Create a new client with an initial set of token IDs.
    ///
    /// Returns the client, a [`NewTokensSender`] for pushing additional token
    /// IDs, and a [`SharedResolutions`] handle that accumulates
    /// `market_resolved` events for the paper loop to drain.
    #[must_use]
    pub fn new(
        token_ids: Vec<String>,
        token_asset_map: SharedTokenAssetMap,
        latest_prices: SharedLatestPrices,
    ) -> (Self, NewTokensSender, SharedResolutions) {
        let (tx, rx) = mpsc::channel(64);
        let resolved = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                token_ids,
                new_tokens_rx: rx,
                resolved_markets: Arc::clone(&resolved),
                token_asset_map,
                latest_prices,
            },
            tx,
            resolved,
        )
    }

    /// Run the WebSocket connection until `shutdown` is cancelled.
    ///
    /// Updates `tracker` on every `best_bid_ask` event. Reconnects with a
    /// 2-second delay after any connection error or server disconnect.
    ///
    /// # Errors
    ///
    /// Errors from individual connection attempts are logged as warnings and
    /// the loop retries. The function only returns `Ok(())` once `shutdown`
    /// is cancelled.
    #[expect(clippy::too_many_lines, reason = "WebSocket run loop is inherently sequential; splitting would obscure control flow")]
    pub async fn run(
        mut self,
        tracker: Arc<Mutex<OrderbookTracker>>,
        shutdown: CancellationToken,
    ) {
        let mut subscribed: Vec<String> = self.token_ids.clone();

        loop {
            if shutdown.is_cancelled() {
                break;
            }

            match connect_async(WS_CLOB_URL).await {
                Ok((ws_stream, _)) => {
                    info!("PM WS connected: {WS_CLOB_URL}");
                    let (mut write, mut read) = ws_stream.split();

                    // Send initial subscribe message.
                    if !subscribed.is_empty() {
                        let sub_msg = build_subscribe_message(&subscribed);
                        if let Err(e) = write.send(Message::Text(sub_msg.into())).await {
                            warn!("PM WS: failed to send subscribe: {e}");
                        } else {
                            debug!(
                                token_count = subscribed.len(),
                                "PM WS: subscribed to tokens"
                            );
                        }
                    }

                    // Heartbeat interval: send PING every 10 seconds.
                    let mut ping_interval =
                        tokio::time::interval(Duration::from_secs(10));
                    ping_interval.tick().await; // consume the immediate first tick

                    let disconnected = loop {
                        tokio::select! {
                            () = shutdown.cancelled() => {
                                return;
                            }

                            _ = ping_interval.tick() => {
                                if let Err(e) = write.send(Message::Text("PING".into())).await {
                                    warn!("PM WS: PING send failed: {e}");
                                    break true;
                                }
                                debug!("PM WS: sent PING");
                            }

                            // Dynamic subscription updates from the scanner.
                            Some(new_ids) = self.new_tokens_rx.recv() => {
                                let mut added = false;
                                for id in new_ids {
                                    if !subscribed.contains(&id) {
                                        subscribed.push(id);
                                        added = true;
                                    }
                                }
                                if added {
                                    let sub_msg = build_subscribe_message(&subscribed);
                                    if let Err(e) = write.send(Message::Text(sub_msg.into())).await {
                                        warn!("PM WS: re-subscribe failed: {e}");
                                        break true;
                                    }
                                    debug!(
                                        token_count = subscribed.len(),
                                        "PM WS: re-subscribed with new tokens"
                                    );
                                }
                            }

                            msg = read.next() => {
                                match msg {
                                    Some(Ok(Message::Text(text))) => {
                                        let text_str: &str = &text;
                                        // Ignore PONG responses.
                                        if text_str == "PONG" {
                                            debug!("PM WS: received PONG");
                                            continue;
                                        }
                                        if let Some(event) = parse_best_bid_ask(text_str) {
                                            handle_best_bid_ask(&tracker, &event);
                                            handle_latest_prices(
                                                &self.token_asset_map,
                                                &self.latest_prices,
                                                &event,
                                            );
                                        } else if let Some(resolution) = parse_market_resolved(text_str) {
                                            info!(
                                                condition_id = %resolution.condition_id,
                                                winning_token = %resolution.winning_token_id,
                                                "PM WS: market_resolved event"
                                            );
                                            if let Ok(mut resolutions) = self.resolved_markets.lock() {
                                                resolutions.push(resolution);
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Ping(data))) => {
                                        // Respond to server-initiated PINGs.
                                        if let Err(e) = write.send(Message::Pong(data)).await {
                                            warn!("PM WS: PONG reply failed: {e}");
                                        }
                                    }
                                    Some(Ok(Message::Close(_))) => {
                                        warn!("PM WS: server sent close frame");
                                        break true;
                                    }
                                    Some(Ok(_)) => {
                                        // Binary/other frames — ignore.
                                    }
                                    Some(Err(e)) => {
                                        warn!("PM WS error: {e}");
                                        break true;
                                    }
                                    None => {
                                        warn!("PM WS: stream ended");
                                        break true;
                                    }
                                }
                            }
                        }
                    };

                    if disconnected && !shutdown.is_cancelled() {
                        warn!("PM WS: disconnected — reconnecting in 2s");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
                Err(e) => {
                    warn!("PM WS connect error: {e}");
                    if !shutdown.is_cancelled() {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        }
    }
}

// ─── Tracker update helper ────────────────────────────────────────────────────

/// Apply a `best_bid_ask` event to the shared tracker.
///
/// Parses the `best_bid` and `best_ask` strings into `f64` and pushes both
/// sides into the tracker as SELL (ask) and BUY (bid) updates respectively.
fn handle_best_bid_ask(
    tracker: &Arc<Mutex<OrderbookTracker>>,
    event: &BestBidAskEvent,
) {
    let timestamp_ms: u64 = event
        .timestamp
        .parse::<f64>()
        .map_or_else(
            |_| {
                #[expect(clippy::cast_possible_truncation, reason = "millis since epoch fits in u64 for centuries")]
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                ms
            },
            |s| {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "timestamp_ms fits in u64")]
                let ms = (s * 1_000.0) as u64;
                ms
            },
        );

    let Ok(best_bid) = event.best_bid.parse::<f64>() else {
        debug!(
            asset_id = %event.asset_id,
            raw = %event.best_bid,
            "PM WS: could not parse best_bid"
        );
        return;
    };
    let Ok(best_ask) = event.best_ask.parse::<f64>() else {
        debug!(
            asset_id = %event.asset_id,
            raw = %event.best_ask,
            "PM WS: could not parse best_ask"
        );
        return;
    };

    let Ok(mut guard) = tracker.lock() else {
        warn!("PM WS: tracker mutex poisoned");
        return;
    };

    guard.update(&event.asset_id, "SELL", best_ask, timestamp_ms);
    guard.update(&event.asset_id, "BUY", best_bid, timestamp_ms);

    debug!(
        asset_id = %event.asset_id,
        market = %event.market,
        bid = best_bid,
        ask = best_ask,
        "PM WS: orderbook update"
    );
}

// ─── LatestPrices update helper ───────────────────────────────────────────────

/// Apply a `best_bid_ask` event to the shared [`LatestPrices`] cache.
///
/// Looks up the token's `(Asset, Timeframe, is_up)` in the shared map, then
/// updates the correct side in the cache. This is the bridge between the WS
/// token-based events and the `(Asset, Timeframe)`-indexed cache consumed by
/// the paper loop.
fn handle_latest_prices(
    token_asset_map: &SharedTokenAssetMap,
    latest_prices: &SharedLatestPrices,
    event: &BestBidAskEvent,
) {
    let Ok(best_bid) = event.best_bid.parse::<f64>() else {
        return;
    };
    let Ok(best_ask) = event.best_ask.parse::<f64>() else {
        return;
    };

    // Sanity-check: skip resolved-market prices.
    if best_ask <= 0.01 || best_ask >= 0.99 || best_bid <= 0.01 || best_bid >= 0.99 {
        return;
    }

    let timestamp_ms: u64 = event
        .timestamp
        .parse::<f64>()
        .map_or_else(
            |_| {
                #[expect(clippy::cast_possible_truncation, reason = "millis since epoch fits in u64")]
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64);
                ms
            },
            |s| {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "timestamp_ms fits in u64")]
                let ms = (s * 1_000.0) as u64;
                ms
            },
        );

    let lookup = if let Ok(map) = token_asset_map.lock() {
        map.get(&event.asset_id).copied()
    } else {
        None
    };

    if let Some((asset, timeframe, is_up)) = lookup {
        if let Ok(mut prices) = latest_prices.lock() {
            prices.update_side(asset, timeframe, is_up, best_bid, best_ask, timestamp_ms);
            debug!(
                asset = %asset,
                timeframe = %timeframe,
                is_up = is_up,
                bid = best_bid,
                ask = best_ask,
                "LatestPrices updated from WS"
            );
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_best_bid_ask_valid() {
        let raw = r#"{"event_type":"best_bid_ask","asset_id":"tok_up","market":"cond_1","best_bid":"0.48","best_ask":"0.52","timestamp":"1774782000"}"#;
        let event = parse_best_bid_ask(raw).expect("should parse best_bid_ask event");
        assert_eq!(event.asset_id, "tok_up");
        assert_eq!(event.market, "cond_1");
        assert_eq!(event.best_bid, "0.48");
        assert_eq!(event.best_ask, "0.52");
    }

    #[test]
    fn parse_best_bid_ask_wrong_event_type() {
        let raw = r#"{"event_type":"price_change","asset_id":"tok_up","market":"cond_1","best_bid":"0.48","best_ask":"0.52"}"#;
        assert!(parse_best_bid_ask(raw).is_none());
    }

    #[test]
    fn parse_best_bid_ask_pong_returns_none() {
        assert!(parse_best_bid_ask("PONG").is_none());
    }

    #[test]
    fn parse_best_bid_ask_invalid_json_returns_none() {
        assert!(parse_best_bid_ask("{not json}").is_none());
    }

    #[test]
    fn build_subscribe_message_contains_token_ids() {
        let ids = vec!["tok_a".to_owned(), "tok_b".to_owned()];
        let msg = build_subscribe_message(&ids);
        assert!(msg.contains("tok_a"));
        assert!(msg.contains("tok_b"));
        assert!(msg.contains("market"));
        assert!(msg.contains("custom_feature_enabled"));
    }

    #[test]
    fn handle_best_bid_ask_updates_tracker() {
        let tracker = Arc::new(Mutex::new(OrderbookTracker::new()));
        {
            let mut guard = tracker.lock().expect("lock");
            guard.register_market("cond_1", "tok_up", "tok_down");
        }

        let event = BestBidAskEvent {
            asset_id: "tok_up".to_owned(),
            market: "cond_1".to_owned(),
            best_bid: "0.48".to_owned(),
            best_ask: "0.52".to_owned(),
            timestamp: "1774782000".to_owned(),
        };

        handle_best_bid_ask(&tracker, &event);

        let guard = tracker.lock().expect("lock");
        let snap = guard.get("cond_1").expect("snapshot should exist");
        let ask = snap.ask_up.expect("ask_up should be set");
        let bid = snap.bid_up.expect("bid_up should be set");
        assert!((ask.as_f64() - 0.52).abs() < 1e-10, "ask_up mismatch");
        assert!((bid.as_f64() - 0.48).abs() < 1e-10, "bid_up mismatch");
    }

    #[test]
    fn parse_market_resolved_valid() {
        let raw = r#"{"event_type":"market_resolved","asset_id":"tok_up","market":"cond_1","timestamp":"1774782000"}"#;
        let res = parse_market_resolved(raw).expect("should parse market_resolved event");
        assert_eq!(res.condition_id, "cond_1");
        assert_eq!(res.winning_token_id, "tok_up");
        assert_eq!(res.timestamp_ms, 1_774_782_000_000);
    }

    #[test]
    fn parse_market_resolved_wrong_event_type() {
        let raw = r#"{"event_type":"best_bid_ask","asset_id":"tok_up","market":"cond_1","best_bid":"0.48","best_ask":"0.52"}"#;
        assert!(parse_market_resolved(raw).is_none());
    }

    #[test]
    fn parse_market_resolved_missing_fields_returns_none() {
        let raw = r#"{"event_type":"market_resolved","asset_id":"","market":"","timestamp":"0"}"#;
        assert!(parse_market_resolved(raw).is_none());
    }

    #[test]
    fn handle_best_bid_ask_invalid_price_is_ignored() {
        let tracker = Arc::new(Mutex::new(OrderbookTracker::new()));
        {
            let mut guard = tracker.lock().expect("lock");
            guard.register_market("cond_1", "tok_up", "tok_down");
        }

        let event = BestBidAskEvent {
            asset_id: "tok_up".to_owned(),
            market: "cond_1".to_owned(),
            best_bid: "not_a_number".to_owned(),
            best_ask: "0.52".to_owned(),
            timestamp: "0".to_owned(),
        };
        handle_best_bid_ask(&tracker, &event);

        let guard = tracker.lock().expect("lock");
        let snap = guard.get("cond_1").expect("snapshot should exist");
        // Neither side should be updated because best_bid is invalid.
        assert!(snap.bid_up.is_none());
        assert!(snap.ask_up.is_none());
    }
}
