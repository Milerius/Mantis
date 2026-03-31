//! Authenticated User WebSocket client for real-time fill monitoring.
//!
//! [`run_user_ws`] connects to the Polymarket User channel and forwards
//! order/trade events onto an unbounded mpsc channel as [`UserWsEvent`]
//! values. It handles reconnection with exponential backoff automatically.

use futures_util::StreamExt;
use polymarket_client_sdk::auth::Credentials;
use polymarket_client_sdk::clob::types::TraderSide;
use polymarket_client_sdk::clob::ws::types::response::{OrderMessageType, WsMessage};
use polymarket_client_sdk::clob::ws::Client as WsClient;
use polymarket_client_sdk::ws::config::Config as WsConfig;
use polymarket_client_sdk::types::Address;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

// ─── Event types ─────────────────────────────────────────────────────────────

/// Events emitted by the User WebSocket client.
#[derive(Debug)]
pub enum UserWsEvent {
    /// An order was updated (fill or cancellation).
    OrderUpdate {
        /// Polymarket order ID.
        order_id: String,
        /// Cumulative shares matched so far.
        size_matched: f64,
        /// `true` when the update represents a cancellation.
        is_cancelled: bool,
    },
    /// A trade was confirmed on-chain.
    TradeConfirmed {
        /// The taker order ID this trade belongs to.
        order_id: String,
        /// Execution price per share.
        price: f64,
        /// Number of shares traded.
        size: f64,
        /// `true` if the authenticated user was the maker side.
        is_maker: bool,
    },
}

/// Sender half of the User WebSocket event channel.
pub type UserWsEventSender = mpsc::UnboundedSender<UserWsEvent>;

/// Receiver half of the User WebSocket event channel.
pub type UserWsEventReceiver = mpsc::UnboundedReceiver<UserWsEvent>;

/// Create a new unbounded channel for User WebSocket events.
pub fn user_ws_channel() -> (UserWsEventSender, UserWsEventReceiver) {
    mpsc::unbounded_channel()
}

// ─── run_user_ws ─────────────────────────────────────────────────────────────

/// Connect to the Polymarket User WebSocket and forward events to `tx`.
///
/// Loops indefinitely, reconnecting with exponential backoff (1 s → 30 s max)
/// on stream errors or unexpected closes. Returns as soon as `shutdown` is
/// cancelled.
///
/// # Parameters
///
/// - `client` — authenticated CLOB client used to subscribe to user events.
/// - `tx` — sender half of the event channel; closed when this task returns.
/// - `shutdown` — cancellation token; when triggered the function exits.
pub async fn run_user_ws(
    credentials: Credentials,
    address: Address,
    tx: UserWsEventSender,
    shutdown: CancellationToken,
) {
    const BACKOFF_INIT_MS: u64 = 1_000;
    const BACKOFF_MAX_MS: u64 = 30_000;
    const WS_ENDPOINT: &str = "wss://ws-subscriptions-clob.polymarket.com";

    let mut backoff_ms = BACKOFF_INIT_MS;

    loop {
        // Create a fresh WS client for each connection attempt.
        let ws_client = match WsClient::new(WS_ENDPOINT, WsConfig::default())
            .and_then(|c| c.authenticate(credentials.clone(), address))
        {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "failed to create user WS client");
                tokio::select! {
                    _ = shutdown.cancelled() => return,
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)) => {}
                }
                backoff_ms = (backoff_ms * 2).min(BACKOFF_MAX_MS);
                continue;
            }
        };

        // Subscribe to all user events across all markets.
        let stream = match ws_client.subscribe_user_events(vec![]) {
            Ok(s) => {
                info!("user WebSocket subscription established");
                backoff_ms = BACKOFF_INIT_MS; // reset on successful connect
                s
            }
            Err(e) => {
                warn!(error = %e, backoff_ms, "failed to subscribe to user events — retrying");
                tokio::select! {
                    _ = shutdown.cancelled() => return,
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)) => {}
                }
                backoff_ms = (backoff_ms * 2).min(BACKOFF_MAX_MS);
                continue;
            }
        };

        tokio::pin!(stream);

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    info!("user WebSocket task shutting down");
                    return;
                }

                maybe_msg = stream.next() => {
                    match maybe_msg {
                        None => {
                            warn!("user WebSocket stream ended unexpectedly — reconnecting");
                            break;
                        }
                        Some(Err(e)) => {
                            warn!(error = %e, "user WebSocket stream error — reconnecting");
                            break;
                        }
                        Some(Ok(msg)) => {
                            handle_message(msg, &tx);
                        }
                    }
                }
            }
        }

        // Reconnect with backoff after inner loop exits.
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)) => {}
        }
        backoff_ms = (backoff_ms * 2).min(BACKOFF_MAX_MS);
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn handle_message(msg: WsMessage, tx: &UserWsEventSender) {
    match msg {
        WsMessage::Order(order) => {
            let is_cancelled = matches!(
                order.msg_type,
                Some(OrderMessageType::Cancellation)
            );
            let size_matched = order
                .size_matched
                .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
                .unwrap_or(0.0);

            debug!(
                order_id = %order.id,
                size_matched,
                is_cancelled,
                "order event received",
            );

            let event = UserWsEvent::OrderUpdate {
                order_id: order.id,
                size_matched,
                is_cancelled,
            };

            if tx.send(event).is_err() {
                debug!("user WebSocket event receiver dropped — stopping");
            }
        }

        WsMessage::Trade(trade) => {
            let price = trade.price.to_string().parse::<f64>().unwrap_or(0.0);
            let size = trade.size.to_string().parse::<f64>().unwrap_or(0.0);
            let is_maker = matches!(trade.trader_side, Some(TraderSide::Maker));
            let order_id = trade.taker_order_id.unwrap_or_else(|| trade.id.clone());

            debug!(
                order_id = %order_id,
                price,
                size,
                is_maker,
                "trade event received",
            );

            let event = UserWsEvent::TradeConfirmed {
                order_id,
                price,
                size,
                is_maker,
            };

            if tx.send(event).is_err() {
                debug!("user WebSocket event receiver dropped — stopping");
            }
        }

        _ => {
            // Market data messages arrive on this channel occasionally;
            // they are not relevant here.
        }
    }
}
