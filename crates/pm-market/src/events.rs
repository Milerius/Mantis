//! Typed event channels for WebSocket-to-main-loop communication.
//!
//! Replaces `Arc<Mutex<...>>` shared state with an `mpsc` channel that
//! the WebSocket task sends through, and the main loop receives from.

use crate::l2_orderbook::{BookLevel, PriceChange};

// ─── PmEvent ────────────────────────────────────────────────────────────────

/// Events from the Polymarket WebSocket to the main loop.
#[derive(Debug, Clone)]
pub enum PmEvent {
    /// Best bid/ask update for a token.
    BestBidAsk {
        /// Token ID this update is for.
        token_id: String,
        /// Best bid price.
        best_bid: f64,
        /// Best ask price.
        best_ask: f64,
        /// Unix timestamp in milliseconds.
        timestamp_ms: u64,
    },
    /// L2 orderbook incremental update (`book` event).
    Book {
        /// Token ID this event applies to.
        token_id: String,
        /// Changed bid levels.
        bids: Vec<BookLevel>,
        /// Changed ask levels.
        asks: Vec<BookLevel>,
        /// Unix timestamp in milliseconds.
        timestamp_ms: u64,
    },
    /// L2 orderbook price change (legacy).
    PriceChange {
        /// Token ID this event applies to.
        token_id: String,
        /// Changed levels.
        changes: Vec<PriceChange>,
        /// Unix timestamp in milliseconds.
        timestamp_ms: u64,
    },
    /// Market resolved.
    MarketResolved {
        /// The condition ID of the resolved market.
        condition_id: String,
        /// The token ID of the winning outcome (Up or Down).
        winning_token_id: String,
        /// Unix timestamp in milliseconds.
        timestamp_ms: u64,
    },
}
