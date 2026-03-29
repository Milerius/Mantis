//! Polymarket WebSocket orderbook tracker.
//!
//! Maintains a live best-bid/ask snapshot for each active market by consuming
//! the Polymarket CLOB WebSocket feed. The tracker is intentionally simple: it
//! only stores the best level on each side, which is all the downstream
//! strategies need to make entry decisions.

use std::collections::HashMap;

use pm_types::ContractPrice;
use serde::Deserialize;
use tracing::debug;

// ─── Public types ────────────────────────────────────────────────────────────

/// Live orderbook state for a single binary market.
///
/// Prices are in `[0.0, 1.0]` as Polymarket contracts are probability-priced.
/// `None` means no quote has been observed yet on that side.
#[derive(Debug, Clone, Copy)]
pub struct OrderbookSnapshot {
    /// Best ask (lowest offer) for the Up contract.
    pub ask_up: Option<ContractPrice>,
    /// Best ask (lowest offer) for the Down contract.
    pub ask_down: Option<ContractPrice>,
    /// Best bid (highest bid) for the Up contract.
    pub bid_up: Option<ContractPrice>,
    /// Best bid (highest bid) for the Down contract.
    pub bid_down: Option<ContractPrice>,
    /// Unix timestamp in milliseconds of the last update.
    pub timestamp_ms: u64,
}

impl OrderbookSnapshot {
    /// Construct an empty snapshot with no quotes.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ask_up: None,
            ask_down: None,
            bid_up: None,
            bid_down: None,
            timestamp_ms: 0,
        }
    }
}

impl Default for OrderbookSnapshot {
    fn default() -> Self {
        Self::empty()
    }
}

// ─── WebSocket message types ─────────────────────────────────────────────────

/// A single price level in a WebSocket orderbook message.
#[derive(Debug, Deserialize)]
pub struct PriceLevel {
    /// Price as a string (Polymarket uses string-encoded decimals).
    pub price: String,
    /// Size at this price level.
    #[allow(dead_code)]
    pub size: String,
}

/// Orderbook update message received from the Polymarket CLOB WebSocket.
#[derive(Debug, Deserialize)]
pub struct OrderbookMessage {
    /// The token ID this update applies to.
    pub asset_id: String,
    /// The market side: `"BUY"` (bids) or `"SELL"` (asks).
    pub market: String,
    /// Changed price levels.
    #[serde(default)]
    pub asks: Vec<PriceLevel>,
    /// Changed price levels.
    #[serde(default)]
    pub bids: Vec<PriceLevel>,
    /// Unix timestamp in milliseconds (string-encoded).
    #[serde(default)]
    pub timestamp: String,
}

// ─── Tracker ─────────────────────────────────────────────────────────────────

/// Tracks live orderbook snapshots for all active markets.
///
/// Markets are indexed in two ways:
/// - by `condition_id` for retrieval by downstream consumers
/// - by `token_id` for routing incoming WebSocket updates
pub struct OrderbookTracker {
    /// Snapshot keyed by condition_id.
    books: HashMap<String, OrderbookSnapshot>,
    /// Maps token_id_up → condition_id.
    token_up_to_condition: HashMap<String, String>,
    /// Maps token_id_down → condition_id.
    token_down_to_condition: HashMap<String, String>,
}

impl OrderbookTracker {
    /// Create an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            books: HashMap::new(),
            token_up_to_condition: HashMap::new(),
            token_down_to_condition: HashMap::new(),
        }
    }

    /// Register a market so the tracker can route token updates to it.
    pub fn register_market(
        &mut self,
        condition_id: &str,
        token_id_up: &str,
        token_id_down: &str,
    ) {
        self.books
            .entry(condition_id.to_owned())
            .or_insert_with(OrderbookSnapshot::empty);
        self.token_up_to_condition
            .insert(token_id_up.to_owned(), condition_id.to_owned());
        self.token_down_to_condition
            .insert(token_id_down.to_owned(), condition_id.to_owned());
    }

    /// Update the best bid or ask for a token.
    ///
    /// `side` should be `"BUY"` for bids or `"SELL"` for asks.
    /// `price` is the best price at the given side. Values outside `[0.0, 1.0]`
    /// are silently ignored (contract prices must be a valid probability).
    pub fn update(&mut self, token_id: &str, side: &str, price: f64, timestamp_ms: u64) {
        let Some(contract_price) = ContractPrice::new(price) else {
            return;
        };

        // Determine which condition_id and which leg (Up vs Down).
        let (condition_id, is_up) = if let Some(cid) = self.token_up_to_condition.get(token_id) {
            (cid.clone(), true)
        } else if let Some(cid) = self.token_down_to_condition.get(token_id) {
            (cid.clone(), false)
        } else {
            debug!(token_id = %token_id, "received update for unknown token");
            return;
        };

        let snapshot = self
            .books
            .entry(condition_id.clone())
            .or_insert_with(OrderbookSnapshot::empty);

        snapshot.timestamp_ms = timestamp_ms;

        match (side.to_uppercase().as_str(), is_up) {
            ("SELL", true) => snapshot.ask_up = Some(contract_price),
            ("SELL", false) => snapshot.ask_down = Some(contract_price),
            ("BUY", true) => snapshot.bid_up = Some(contract_price),
            ("BUY", false) => snapshot.bid_down = Some(contract_price),
            _ => {
                debug!(side = %side, "unrecognised orderbook side");
            }
        }
    }

    /// Get the current snapshot for a market by its condition ID.
    ///
    /// Returns `None` if no snapshot has been registered for this market.
    #[must_use]
    pub fn get(&self, condition_id: &str) -> Option<&OrderbookSnapshot> {
        self.books.get(condition_id)
    }
}

impl Default for OrderbookTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── WebSocket subscription helpers ─────────────────────────────────────────

/// Polymarket CLOB WebSocket URL.
pub const WS_CLOB_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";

/// Serialise a subscribe message for the given token IDs.
///
/// The returned string is ready to send as a WebSocket text frame.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] if serialisation fails (should never happen
/// in practice as the input is a simple string slice).
pub fn subscribe_message(asset_ids: &[&str]) -> Result<String, serde_json::Error> {
    let payload = serde_json::json!({
        "type": "subscribe",
        "channel": "market",
        "assets_id": asset_ids,
    });
    serde_json::to_string(&payload)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── OrderbookTracker: update and get ──────────────────────────────────────

    #[test]
    fn tracker_missing_market_returns_none() {
        let tracker = OrderbookTracker::new();
        assert!(tracker.get("nonexistent").is_none());
    }

    #[test]
    fn tracker_registered_market_starts_empty() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        let snap = tracker.get("cond_1").expect("registered market should exist");
        assert!(snap.ask_up.is_none());
        assert!(snap.ask_down.is_none());
        assert!(snap.bid_up.is_none());
        assert!(snap.bid_down.is_none());
        assert_eq!(snap.timestamp_ms, 0);
    }

    #[test]
    fn tracker_update_ask_up() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        tracker.update("tok_up", "SELL", 0.55, 1_000);

        let snap = tracker.get("cond_1").expect("should exist");
        let ask = snap.ask_up.expect("ask_up should be set");
        assert!((ask.as_f64() - 0.55).abs() < 1e-10);
        assert_eq!(snap.timestamp_ms, 1_000);
    }

    #[test]
    fn tracker_update_ask_down() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        tracker.update("tok_down", "SELL", 0.48, 2_000);

        let snap = tracker.get("cond_1").expect("should exist");
        let ask = snap.ask_down.expect("ask_down should be set");
        assert!((ask.as_f64() - 0.48).abs() < 1e-10);
    }

    #[test]
    fn tracker_update_bid_up() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        tracker.update("tok_up", "BUY", 0.52, 3_000);

        let snap = tracker.get("cond_1").expect("should exist");
        let bid = snap.bid_up.expect("bid_up should be set");
        assert!((bid.as_f64() - 0.52).abs() < 1e-10);
    }

    #[test]
    fn tracker_update_bid_down() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        tracker.update("tok_down", "BUY", 0.45, 4_000);

        let snap = tracker.get("cond_1").expect("should exist");
        let bid = snap.bid_down.expect("bid_down should be set");
        assert!((bid.as_f64() - 0.45).abs() < 1e-10);
    }

    #[test]
    fn tracker_update_unknown_token_is_ignored() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        // Should not panic or alter state.
        tracker.update("unknown_token", "SELL", 0.50, 5_000);
        let snap = tracker.get("cond_1").expect("should exist");
        assert!(snap.ask_up.is_none());
    }

    #[test]
    fn tracker_update_price_out_of_range_is_ignored() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        // Price > 1.0 is invalid for a binary contract.
        tracker.update("tok_up", "SELL", 1.5, 6_000);
        let snap = tracker.get("cond_1").expect("should exist");
        assert!(snap.ask_up.is_none());
    }

    #[test]
    fn tracker_side_case_insensitive() {
        let mut tracker = OrderbookTracker::new();
        tracker.register_market("cond_1", "tok_up", "tok_down");
        tracker.update("tok_up", "sell", 0.55, 7_000);

        let snap = tracker.get("cond_1").expect("should exist");
        assert!(snap.ask_up.is_some());
    }

    #[test]
    fn subscribe_message_contains_assets() {
        let msg = subscribe_message(&["tok_a", "tok_b"]).expect("serialise should succeed");
        assert!(msg.contains("subscribe"));
        assert!(msg.contains("tok_a"));
        assert!(msg.contains("tok_b"));
    }
}
