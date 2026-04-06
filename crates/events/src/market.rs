//! Market data event payloads and supporting enums.

use core::fmt;

use mantis_types::{Lots, Side, Ticks};

/// The action applied to an order-book level or trade entry.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum UpdateAction {
    /// A new level or entry is being added.
    New = 0,
    /// An existing level or entry is being modified.
    Change = 1,
    /// An existing level or entry is being removed.
    Delete = 2,
}

impl fmt::Debug for UpdateAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::New => write!(f, "UpdateAction::New"),
            Self::Change => write!(f, "UpdateAction::Change"),
            Self::Delete => write!(f, "UpdateAction::Delete"),
        }
    }
}

/// A single incremental book-level change.
///
/// Carries the price, quantity, side, action, and depth index for one
/// update to the consolidated order book.
#[derive(Clone, Copy)]
#[repr(C)]
#[expect(clippy::pub_underscore_fields, reason = "repr(C) padding field")]
pub struct BookDeltaPayload {
    /// Price of the updated level, in ticks.
    pub price: Ticks,
    /// Quantity at the updated level, in lots.
    pub qty: Lots,
    /// Which side of the book this level belongs to.
    pub side: Side,
    /// What happened to this level (add / change / remove).
    pub action: UpdateAction,
    /// Zero-based depth index (0 = best bid/ask).
    pub depth: u8,
    /// Reserved padding — must be zero.
    pub _pad: [u8; 5],
}

/// A single matched trade.
///
/// Represents a completed trade as seen from the market feed.
#[derive(Clone, Copy)]
#[repr(C)]
#[expect(clippy::pub_underscore_fields, reason = "repr(C) padding field")]
pub struct TradePayload {
    /// Execution price in ticks.
    pub price: Ticks,
    /// Executed quantity in lots.
    pub qty: Lots,
    /// Side of the aggressing (taker) order.
    pub aggressor: Side,
    /// Reserved padding — must be zero.
    pub _pad: [u8; 7],
}

/// Best bid and offer snapshot.
///
/// Captures the full top-of-book state at a single point in time.
///
/// A future `IS_DERIVED` flag in `EventFlags` is anticipated to distinguish
/// direct-from-venue vs engine-derived `TopOfBook` events.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct TopOfBookPayload {
    /// Best bid price in ticks.
    pub bid_price: Ticks,
    /// Best bid quantity in lots.
    pub bid_qty: Lots,
    /// Best ask price in ticks.
    pub ask_price: Ticks,
    /// Best ask quantity in lots.
    pub ask_qty: Lots,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn update_action_repr_values() {
        assert_eq!(UpdateAction::New as u8, 0);
        assert_eq!(UpdateAction::Change as u8, 1);
        assert_eq!(UpdateAction::Delete as u8, 2);
    }

    #[test]
    fn update_action_size_is_1() {
        assert_eq!(mem::size_of::<UpdateAction>(), 1);
    }

    #[test]
    fn book_delta_payload_size_is_24() {
        assert_eq!(mem::size_of::<BookDeltaPayload>(), 24);
    }

    #[test]
    fn trade_payload_size_is_24() {
        assert_eq!(mem::size_of::<TradePayload>(), 24);
    }

    #[test]
    fn top_of_book_payload_size_is_32() {
        assert_eq!(mem::size_of::<TopOfBookPayload>(), 32);
    }

    #[test]
    fn book_delta_roundtrip() {
        let p = BookDeltaPayload {
            price: Ticks::from_raw(100),
            qty: Lots::from_raw(10),
            side: Side::Bid,
            action: UpdateAction::New,
            depth: 0,
            _pad: [0; 5],
        };
        assert_eq!(p.price, Ticks::from_raw(100));
        assert_eq!(p.qty, Lots::from_raw(10));
        assert_eq!(p.side, Side::Bid);
        assert_eq!(p.action, UpdateAction::New);
        assert_eq!(p.depth, 0);
    }

    #[test]
    fn trade_roundtrip() {
        let p = TradePayload {
            price: Ticks::from_raw(200),
            qty: Lots::from_raw(5),
            aggressor: Side::Ask,
            _pad: [0; 7],
        };
        assert_eq!(p.price, Ticks::from_raw(200));
        assert_eq!(p.qty, Lots::from_raw(5));
        assert_eq!(p.aggressor, Side::Ask);
    }

    #[test]
    fn top_of_book_roundtrip() {
        let p = TopOfBookPayload {
            bid_price: Ticks::from_raw(99),
            bid_qty: Lots::from_raw(50),
            ask_price: Ticks::from_raw(101),
            ask_qty: Lots::from_raw(30),
        };
        assert_eq!(p.bid_price, Ticks::from_raw(99));
        assert_eq!(p.bid_qty, Lots::from_raw(50));
        assert_eq!(p.ask_price, Ticks::from_raw(101));
        assert_eq!(p.ask_qty, Lots::from_raw(30));
    }

    // --- mutant-catching: field distinctness ---

    #[test]
    fn book_delta_ask_side() {
        let p = BookDeltaPayload {
            price: Ticks::from_raw(200),
            qty: Lots::from_raw(50),
            side: Side::Ask,
            action: UpdateAction::Change,
            depth: 1,
            _pad: [0; 5],
        };
        assert_eq!(p.price, Ticks::from_raw(200));
        assert_eq!(p.qty, Lots::from_raw(50));
        assert_eq!(p.side, Side::Ask);
        assert_eq!(p.action, UpdateAction::Change);
        assert_eq!(p.depth, 1);
    }

    #[test]
    fn book_delta_delete_action() {
        let p = BookDeltaPayload {
            price: Ticks::from_raw(100),
            qty: Lots::ZERO,
            side: Side::Bid,
            action: UpdateAction::Delete,
            depth: 0,
            _pad: [0; 5],
        };
        assert_eq!(p.action, UpdateAction::Delete);
        assert_eq!(p.qty, Lots::ZERO);
    }

    #[test]
    fn trade_bid_aggressor() {
        let p = TradePayload {
            price: Ticks::from_raw(150),
            qty: Lots::from_raw(10),
            aggressor: Side::Bid,
            _pad: [0; 7],
        };
        assert_eq!(p.aggressor, Side::Bid);
        assert_eq!(p.price, Ticks::from_raw(150));
        assert_eq!(p.qty, Lots::from_raw(10));
    }

    #[test]
    fn update_action_change_distinct_from_new_and_delete() {
        assert_ne!(UpdateAction::Change as u8, UpdateAction::New as u8);
        assert_ne!(UpdateAction::Change as u8, UpdateAction::Delete as u8);
    }
}
