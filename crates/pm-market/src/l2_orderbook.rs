//! L2 orderbook reconstruction from Polymarket CLOB WebSocket `price_change` events.
//!
//! Maintains full depth on both sides (bids and asks) for each subscribed token.
//! This is an additional data source alongside the existing [`LatestPrices`] cache
//! which only tracks top-of-book.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

// ─── OrderbookSide ──────────────────────────────────────────────────────────

/// A single side (bids or asks) of an L2 orderbook.
///
/// Keys are price levels (as integer cents, e.g. 50 = $0.50), values are total
/// size at that level.
#[derive(Debug, Clone, Default)]
pub struct OrderbookSide {
    /// Price (in cents 0–100) → total size at that price level.
    levels: BTreeMap<u32, f64>,
}

impl OrderbookSide {
    /// Set the size at a given price level. Removes the level if `new_size` is
    /// zero or negative.
    pub fn update(&mut self, price_cents: u32, new_size: f64) {
        if new_size <= 0.0 {
            self.levels.remove(&price_cents);
        } else {
            self.levels.insert(price_cents, new_size);
        }
    }

    /// Best price and size on this side.
    ///
    /// For bids this is the **highest** price; for asks it is the **lowest**.
    /// The caller must choose which end to read — this method returns the
    /// **last** (highest) entry, suitable for the bid side. Use
    /// [`best_ask`](L2Orderbook::best_ask) which reads the first (lowest) entry
    /// for the ask side.
    #[must_use]
    fn best_high(&self) -> Option<(u32, f64)> {
        self.levels.iter().next_back().map(|(&p, &s)| (p, s))
    }

    /// Best price and size — returns the **first** (lowest) entry, suitable
    /// for the ask side.
    #[must_use]
    fn best_low(&self) -> Option<(u32, f64)> {
        self.levels.iter().next().map(|(&p, &s)| (p, s))
    }

    /// Total size across the top `n` levels (ordered from best).
    ///
    /// For bids, "top" means highest prices; for asks, lowest prices. The
    /// `from_high` parameter controls iteration order.
    #[must_use]
    fn depth_directed(&self, n: usize, from_high: bool) -> f64 {
        if from_high {
            self.levels.values().rev().take(n).sum()
        } else {
            self.levels.values().take(n).sum()
        }
    }

    /// Sum of all sizes across every level.
    #[must_use]
    pub fn total_depth(&self) -> f64 {
        self.levels.values().sum()
    }

    /// Remove all levels.
    pub fn clear(&mut self) {
        self.levels.clear();
    }

    /// Number of non-zero levels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Whether there are no levels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }
}

// ─── L2Orderbook ────────────────────────────────────────────────────────────

/// Full L2 orderbook for a single token (Up or Down).
#[derive(Debug, Clone, Default)]
pub struct L2Orderbook {
    /// Bid side — buyers, best = highest price.
    pub bids: OrderbookSide,
    /// Ask side — sellers, best = lowest price.
    pub asks: OrderbookSide,
    /// Timestamp of the last update (Unix milliseconds).
    pub timestamp_ms: u64,
}

/// Convert a decimal price (e.g. 0.50) to integer cents (50).
///
/// Clamps to `[0, 100]`.
#[must_use]
fn price_to_cents(price: f64) -> u32 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "price is clamped to [0, 100] range before cast"
    )]
    let cents = (price * 100.0).round().clamp(0.0, 100.0) as u32;
    cents
}

/// Convert integer cents back to a decimal price.
#[must_use]
fn cents_to_price(cents: u32) -> f64 {
    f64::from(cents) / 100.0
}

impl L2Orderbook {
    /// Update a bid level.
    pub fn update_bid(&mut self, price_cents: u32, size: f64, ts: u64) {
        self.bids.update(price_cents, size);
        self.timestamp_ms = ts;
    }

    /// Update an ask level.
    pub fn update_ask(&mut self, price_cents: u32, size: f64, ts: u64) {
        self.asks.update(price_cents, size);
        self.timestamp_ms = ts;
    }

    /// Best bid price as a decimal (e.g. 0.50).
    #[must_use]
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.best_high().map(|(c, _)| cents_to_price(c))
    }

    /// Best ask price as a decimal (e.g. 0.52).
    #[must_use]
    pub fn best_ask(&self) -> Option<f64> {
        self.asks.best_low().map(|(c, _)| cents_to_price(c))
    }

    /// Spread = best ask - best bid. `None` if either side is empty.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some(ask - bid),
            _ => None,
        }
    }

    /// Orderbook imbalance at the top `depth_levels` levels.
    ///
    /// `(bid_depth - ask_depth) / (bid_depth + ask_depth)`, range `[-1, 1]`.
    /// Positive = buy pressure (bullish), negative = sell pressure (bearish).
    /// Returns `0.0` if both sides are empty.
    #[must_use]
    pub fn imbalance(&self, depth_levels: usize) -> f64 {
        let bid_depth = self.bids.depth_directed(depth_levels, true);
        let ask_depth = self.asks.depth_directed(depth_levels, false);
        let total = bid_depth + ask_depth;
        if total == 0.0 {
            return 0.0;
        }
        (bid_depth - ask_depth) / total
    }

    /// Estimate the average fill price for a given order size in USDC.
    ///
    /// `"BUY"` walks the ask side (lowest to highest); `"SELL"` walks the bid
    /// side (highest to lowest). Returns the volume-weighted average fill price,
    /// or `0.0` if the book is empty.
    #[must_use]
    pub fn slippage_for_size(&self, side: &str, size_usdc: f64) -> f64 {
        if size_usdc <= 0.0 {
            return 0.0;
        }

        match side.to_uppercase().as_str() {
            "BUY" => walk_asks(&self.asks, size_usdc),
            "SELL" => walk_bids(&self.bids, size_usdc),
            _ => 0.0,
        }
    }
}

/// Walk the ask side (lowest price first) filling `remaining` USDC.
fn walk_asks(asks: &OrderbookSide, mut remaining: f64) -> f64 {
    let mut total_cost = 0.0;
    let mut total_qty = 0.0;

    for (&price_cents, &size) in &asks.levels {
        if remaining <= 0.0 {
            break;
        }
        let price = cents_to_price(price_cents);
        // size is in contracts; cost per contract = price
        let available_cost = size * price;
        let fill_cost = available_cost.min(remaining);
        let fill_qty = if price > 0.0 { fill_cost / price } else { 0.0 };

        total_cost += fill_cost;
        total_qty += fill_qty;
        remaining -= fill_cost;
    }

    if total_qty > 0.0 {
        total_cost / total_qty
    } else {
        0.0
    }
}

/// Walk the bid side (highest price first) filling `remaining` USDC.
fn walk_bids(bids: &OrderbookSide, mut remaining: f64) -> f64 {
    let mut total_cost = 0.0;
    let mut total_qty = 0.0;

    for (&price_cents, &size) in bids.levels.iter().rev() {
        if remaining <= 0.0 {
            break;
        }
        let price = cents_to_price(price_cents);
        let available_cost = size * price;
        let fill_cost = available_cost.min(remaining);
        let fill_qty = if price > 0.0 { fill_cost / price } else { 0.0 };

        total_cost += fill_cost;
        total_qty += fill_qty;
        remaining -= fill_cost;
    }

    if total_qty > 0.0 {
        total_cost / total_qty
    } else {
        0.0
    }
}

// ─── PriceChange ────────────────────────────────────────────────────────────

/// A single level change from a `price_change` WebSocket event.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceChange {
    /// Price as a string-encoded decimal (e.g. `"0.50"`).
    pub price: String,
    /// New total size at this level (string-encoded; `"0"` = remove).
    pub size: String,
    /// `"BUY"` (bid) or `"SELL"` (ask).
    pub side: String,
}

/// Envelope for a `price_change` WebSocket event.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceChangeEvent {
    /// Always `"price_change"`.
    pub event_type: String,
    /// Token ID this event applies to.
    pub asset_id: String,
    /// Changed levels.
    #[serde(default)]
    pub changes: Vec<PriceChange>,
    /// Unix timestamp (string-encoded seconds).
    #[serde(default)]
    pub timestamp: String,
}

// ─── L2OrderbookManager ─────────────────────────────────────────────────────

/// Manages L2 orderbooks for all subscribed tokens.
pub struct L2OrderbookManager {
    /// `token_id` → `L2Orderbook`.
    books: HashMap<String, L2Orderbook>,
}

impl L2OrderbookManager {
    /// Create an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            books: HashMap::new(),
        }
    }

    /// Get the orderbook for a given token, if it exists.
    #[must_use]
    pub fn get_book(&self, token_id: &str) -> Option<&L2Orderbook> {
        self.books.get(token_id)
    }

    /// Process a `book` event (L2 incremental update) for a token.
    ///
    /// Creates the book on first update.
    pub fn process_book_event(
        &mut self,
        token_id: &str,
        event: &BookEvent,
        timestamp_ms: u64,
    ) {
        let book = self
            .books
            .entry(token_id.to_owned())
            .or_default();

        for bid in &event.bids {
            let Ok(price) = bid.price.parse::<f64>() else { continue };
            let Ok(size) = bid.size.parse::<f64>() else { continue };
            book.update_bid(price_to_cents(price), size, timestamp_ms);
        }
        for ask in &event.asks {
            let Ok(price) = ask.price.parse::<f64>() else { continue };
            let Ok(size) = ask.size.parse::<f64>() else { continue };
            book.update_ask(price_to_cents(price), size, timestamp_ms);
        }
    }

    /// Process a batch of price changes for a token.
    ///
    /// Creates the book on first update.
    pub fn process_price_change(
        &mut self,
        token_id: &str,
        changes: &[PriceChange],
        timestamp_ms: u64,
    ) {
        let book = self
            .books
            .entry(token_id.to_owned())
            .or_default();

        for change in changes {
            let Ok(price) = change.price.parse::<f64>() else {
                continue;
            };
            let Ok(size) = change.size.parse::<f64>() else {
                continue;
            };
            let cents = price_to_cents(price);

            match change.side.to_uppercase().as_str() {
                "BUY" => book.update_bid(cents, size, timestamp_ms),
                "SELL" => book.update_ask(cents, size, timestamp_ms),
                _ => {}
            }
        }
    }
}

impl Default for L2OrderbookManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Shared type alias ─────────────────────────────────────────────────────

/// Thread-safe shared handle to the L2 orderbook manager.
pub type SharedL2Manager = std::sync::Arc<std::sync::Mutex<L2OrderbookManager>>;

// ─── Book Event (L2 incremental update) ────────────────────────────────────

/// A single level from a `book` WebSocket event.
#[derive(Debug, Clone, Deserialize)]
pub struct BookLevel {
    /// Price as string-encoded decimal (e.g. `"0.50"`).
    pub price: String,
    /// New total size at this level (string-encoded; `"0"` = remove).
    pub size: String,
}

/// Envelope for a `book` WebSocket event — the actual L2 incremental update.
///
/// Format from Polymarket WS:
/// ```json
/// {"event_type":"book","asset_id":"TOKEN_ID",
///  "bids":[{"price":"0.48","size":"100"}],
///  "asks":[{"price":"0.52","size":"200"}],
///  "timestamp":"1774782000","hash":"..."}
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct BookEvent {
    /// Always `"book"`.
    pub event_type: String,
    /// Token ID this event applies to.
    pub asset_id: String,
    /// Changed bid levels.
    #[serde(default)]
    pub bids: Vec<BookLevel>,
    /// Changed ask levels.
    #[serde(default)]
    pub asks: Vec<BookLevel>,
    /// Unix timestamp (string-encoded seconds).
    #[serde(default)]
    pub timestamp: String,
}

// ─── Parsing helpers ────────────────────────────────────────────────────────

/// Parse a raw WebSocket text message as a `book` event (L2 incremental update).
///
/// Returns `None` for non-`book` events or parse failures.
#[must_use]
pub fn parse_book_event(raw: &str) -> Option<BookEvent> {
    // Fast-path rejection: avoid full JSON parse for non-book messages.
    if !raw.contains("\"book\"") {
        return None;
    }
    let event: BookEvent = serde_json::from_str(raw).ok()?;
    if event.event_type != "book" {
        return None;
    }
    Some(event)
}

/// Parse a raw WebSocket text message as a `price_change` event.
///
/// Returns `None` for non-`price_change` events or parse failures.
#[must_use]
pub fn parse_price_change(raw: &str) -> Option<PriceChangeEvent> {
    if !raw.contains("price_change") {
        return None;
    }
    let event: PriceChangeEvent = serde_json::from_str(raw).ok()?;
    if event.event_type != "price_change" {
        return None;
    }
    Some(event)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── OrderbookSide ────────────────────────────────────────────────────────

    #[test]
    fn side_update_and_remove() {
        let mut side = OrderbookSide::default();
        side.update(50, 100.0);
        assert_eq!(side.len(), 1);
        assert!((side.total_depth() - 100.0).abs() < 1e-10);

        // Remove by setting size to 0.
        side.update(50, 0.0);
        assert!(side.is_empty());
        assert!((side.total_depth()).abs() < 1e-10);
    }

    #[test]
    fn side_overwrite() {
        let mut side = OrderbookSide::default();
        side.update(50, 100.0);
        side.update(50, 200.0);
        assert_eq!(side.len(), 1);
        assert!((side.total_depth() - 200.0).abs() < 1e-10);
    }

    // ── L2Orderbook: best_bid / best_ask ────────────────────────────────────

    #[test]
    fn best_bid_returns_highest() {
        let mut book = L2Orderbook::default();
        book.update_bid(48, 10.0, 1000);
        book.update_bid(50, 20.0, 1000);
        book.update_bid(49, 15.0, 1000);
        assert!((book.best_bid().expect("bid") - 0.50).abs() < 1e-10);
    }

    #[test]
    fn best_ask_returns_lowest() {
        let mut book = L2Orderbook::default();
        book.update_ask(52, 10.0, 1000);
        book.update_ask(51, 20.0, 1000);
        book.update_ask(53, 5.0, 1000);
        assert!((book.best_ask().expect("ask") - 0.51).abs() < 1e-10);
    }

    #[test]
    fn empty_book_returns_none() {
        let book = L2Orderbook::default();
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
        assert!(book.spread().is_none());
    }

    // ── spread ──────────────────────────────────────────────────────────────

    #[test]
    fn spread_calculation() {
        let mut book = L2Orderbook::default();
        book.update_bid(48, 10.0, 1000);
        book.update_ask(52, 10.0, 1000);
        let spread = book.spread().expect("spread");
        assert!((spread - 0.04).abs() < 1e-10);
    }

    // ── imbalance ───────────────────────────────────────────────────────────

    #[test]
    fn imbalance_all_bids() {
        let mut book = L2Orderbook::default();
        book.update_bid(50, 100.0, 1000);
        book.update_bid(49, 100.0, 1000);
        // No asks → imbalance = (200 - 0) / (200 + 0) = 1.0
        assert!((book.imbalance(5) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn imbalance_all_asks() {
        let mut book = L2Orderbook::default();
        book.update_ask(51, 100.0, 1000);
        book.update_ask(52, 100.0, 1000);
        // No bids → imbalance = (0 - 200) / (0 + 200) = -1.0
        assert!((book.imbalance(5) - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn imbalance_equal() {
        let mut book = L2Orderbook::default();
        book.update_bid(50, 100.0, 1000);
        book.update_ask(51, 100.0, 1000);
        assert!((book.imbalance(5)).abs() < 1e-10);
    }

    #[test]
    fn imbalance_empty_is_zero() {
        let book = L2Orderbook::default();
        assert!((book.imbalance(5)).abs() < 1e-10);
    }

    #[test]
    fn imbalance_respects_depth_levels() {
        let mut book = L2Orderbook::default();
        // 3 bid levels
        book.update_bid(50, 100.0, 1000);
        book.update_bid(49, 100.0, 1000);
        book.update_bid(48, 100.0, 1000);
        // 1 ask level
        book.update_ask(51, 50.0, 1000);

        // Top 1 level: bid=100 vs ask=50 → (100-50)/(100+50) = 1/3
        let imb1 = book.imbalance(1);
        assert!((imb1 - 1.0 / 3.0).abs() < 1e-10, "got {imb1}");

        // Top 3 levels: bid=300 vs ask=50 → (300-50)/(300+50) = 250/350
        let imb3 = book.imbalance(3);
        assert!((imb3 - 250.0 / 350.0).abs() < 1e-10, "got {imb3}");
    }

    // ── slippage_for_size ───────────────────────────────────────────────────

    #[test]
    fn slippage_buy_single_level() {
        let mut book = L2Orderbook::default();
        // 100 contracts at $0.50 = $50 available
        book.update_ask(50, 100.0, 1000);
        // Buy $25 → all fills at 0.50
        let avg = book.slippage_for_size("BUY", 25.0);
        assert!((avg - 0.50).abs() < 1e-10, "got {avg}");
    }

    #[test]
    fn slippage_buy_walks_multiple_levels() {
        let mut book = L2Orderbook::default();
        // 100 contracts at $0.50 ($50 available)
        book.update_ask(50, 100.0, 1000);
        // 100 contracts at $0.52 ($52 available)
        book.update_ask(52, 100.0, 1000);

        // Buy $75: fill $50 at 0.50, then $25 at 0.52
        // VWAP = (50 + 25) / (100 + 25/0.52) = 75 / (100 + 48.077) = 75/148.077
        let avg = book.slippage_for_size("BUY", 75.0);
        let expected = 75.0 / (100.0 + 25.0 / 0.52);
        assert!(
            (avg - expected).abs() < 1e-6,
            "got {avg}, expected {expected}"
        );
    }

    #[test]
    fn slippage_sell_walks_bids_highest_first() {
        let mut book = L2Orderbook::default();
        book.update_bid(50, 100.0, 1000);
        book.update_bid(48, 100.0, 1000);

        // Sell $25: fills at 0.50 first (highest bid)
        let avg = book.slippage_for_size("SELL", 25.0);
        assert!((avg - 0.50).abs() < 1e-10, "got {avg}");
    }

    #[test]
    fn slippage_empty_book_returns_zero() {
        let book = L2Orderbook::default();
        assert!((book.slippage_for_size("BUY", 100.0)).abs() < 1e-10);
    }

    #[test]
    fn slippage_zero_size_returns_zero() {
        let mut book = L2Orderbook::default();
        book.update_ask(50, 100.0, 1000);
        assert!((book.slippage_for_size("BUY", 0.0)).abs() < 1e-10);
    }

    // ── remove level ────────────────────────────────────────────────────────

    #[test]
    fn removing_level_updates_best() {
        let mut book = L2Orderbook::default();
        book.update_bid(50, 100.0, 1000);
        book.update_bid(48, 50.0, 1000);
        assert!((book.best_bid().expect("bid") - 0.50).abs() < 1e-10);

        // Remove the best bid
        book.update_bid(50, 0.0, 2000);
        assert!((book.best_bid().expect("bid") - 0.48).abs() < 1e-10);
    }

    // ── L2OrderbookManager ──────────────────────────────────────────────────

    #[test]
    fn manager_process_price_change() {
        let mut mgr = L2OrderbookManager::new();
        let changes = vec![
            PriceChange {
                price: "0.50".to_owned(),
                size: "100.5".to_owned(),
                side: "BUY".to_owned(),
            },
            PriceChange {
                price: "0.52".to_owned(),
                size: "200.0".to_owned(),
                side: "SELL".to_owned(),
            },
        ];

        mgr.process_price_change("token_1", &changes, 5000);

        let book = mgr.get_book("token_1").expect("book should exist");
        assert!((book.best_bid().expect("bid") - 0.50).abs() < 1e-10);
        assert!((book.best_ask().expect("ask") - 0.52).abs() < 1e-10);
        assert_eq!(book.timestamp_ms, 5000);
    }

    #[test]
    fn manager_remove_level() {
        let mut mgr = L2OrderbookManager::new();
        let add = vec![PriceChange {
            price: "0.50".to_owned(),
            size: "100.0".to_owned(),
            side: "BUY".to_owned(),
        }];
        mgr.process_price_change("token_1", &add, 1000);

        let remove = vec![PriceChange {
            price: "0.50".to_owned(),
            size: "0".to_owned(),
            side: "BUY".to_owned(),
        }];
        mgr.process_price_change("token_1", &remove, 2000);

        let book = mgr.get_book("token_1").expect("book should exist");
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn manager_empty_returns_none() {
        let mgr = L2OrderbookManager::new();
        assert!(mgr.get_book("nonexistent").is_none());
    }

    // ── parse_price_change ──────────────────────────────────────────────────

    #[test]
    fn parse_price_change_valid() {
        let raw = r#"{"event_type":"price_change","asset_id":"tok_1","changes":[{"price":"0.50","size":"100.5","side":"BUY"},{"price":"0.51","size":"0","side":"SELL"}],"timestamp":"1234567890"}"#;
        let event = parse_price_change(raw).expect("should parse price_change");
        assert_eq!(event.asset_id, "tok_1");
        assert_eq!(event.changes.len(), 2);
        assert_eq!(event.changes[0].price, "0.50");
        assert_eq!(event.changes[0].size, "100.5");
        assert_eq!(event.changes[0].side, "BUY");
        assert_eq!(event.changes[1].size, "0");
    }

    #[test]
    fn parse_price_change_wrong_event_type() {
        let raw = r#"{"event_type":"best_bid_ask","asset_id":"tok_1","best_bid":"0.48","best_ask":"0.52"}"#;
        assert!(parse_price_change(raw).is_none());
    }

    #[test]
    fn parse_price_change_not_json() {
        assert!(parse_price_change("PONG").is_none());
    }

    // ── price_to_cents / cents_to_price ─────────────────────────────────────

    #[test]
    fn price_cents_roundtrip() {
        assert_eq!(price_to_cents(0.50), 50);
        assert_eq!(price_to_cents(0.01), 1);
        assert_eq!(price_to_cents(0.99), 99);
        assert_eq!(price_to_cents(1.0), 100);
        assert_eq!(price_to_cents(0.0), 0);

        assert!((cents_to_price(50) - 0.50).abs() < 1e-10);
        assert!((cents_to_price(1) - 0.01).abs() < 1e-10);
    }

    // ── OrderbookSide::clear ────────────────────────────────────────────────

    #[test]
    fn side_clear() {
        let mut side = OrderbookSide::default();
        side.update(50, 100.0);
        side.update(51, 200.0);
        assert_eq!(side.len(), 2);
        side.clear();
        assert!(side.is_empty());
    }
}
