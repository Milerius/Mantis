//! Discriminated event body and standalone kind discriminant.

use core::fmt;

use crate::control::{HeartbeatPayload, TimerPayload};
use crate::execution::{FillPayload, OrderAckPayload, OrderRejectPayload};
use crate::market::{BookDeltaPayload, TopOfBookPayload, TradePayload};

/// Discriminated event payload.
///
/// `repr(C, u16)` gives a 2-byte discriminant stored at offset 0, followed by
/// alignment padding, then the variant data. Layout is verified empirically
/// through const assertions and mantis-layout tests.
///
/// Every variant of `EventBody` has a strict 1:1 correspondence with a
/// variant of [`EventKind`]. Adding a variant to one without updating the
/// other is a compile error enforced by the exhaustive `match` in
/// [`EventBody::kind`].
#[derive(Clone, Copy)]
#[repr(C, u16)]
pub enum EventBody {
    /// An incremental order-book level change.
    BookDelta(BookDeltaPayload),
    /// A single matched trade.
    Trade(TradePayload),
    /// A best-bid/offer snapshot.
    TopOfBook(TopOfBookPayload),
    /// Acknowledgement that an order was accepted or updated.
    OrderAck(OrderAckPayload),
    /// A partial or full fill on an order.
    Fill(FillPayload),
    /// Notification that an order was rejected.
    OrderReject(OrderRejectPayload),
    /// A timer expiry.
    Timer(TimerPayload),
    /// An internal liveness ping.
    Heartbeat(HeartbeatPayload),
}

impl EventBody {
    /// Returns the [`EventKind`] discriminant for this body variant.
    ///
    /// This is a `const fn` exhaustive match with no wildcard arm — adding a
    /// variant to `EventBody` without updating this function is a compile
    /// error, preserving the 1:1 mapping invariant.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::BookDelta(_) => EventKind::BookDelta,
            Self::Trade(_) => EventKind::Trade,
            Self::TopOfBook(_) => EventKind::TopOfBook,
            Self::OrderAck(_) => EventKind::OrderAck,
            Self::Fill(_) => EventKind::Fill,
            Self::OrderReject(_) => EventKind::OrderReject,
            Self::Timer(_) => EventKind::Timer,
            Self::Heartbeat(_) => EventKind::Heartbeat,
        }
    }
}

/// Standalone discriminant for [`EventBody`].
///
/// Must stay in strict 1:1 correspondence with `EventBody` variants.
/// The numeric values are stable across releases — do not reorder or remove.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum EventKind {
    /// Corresponds to [`EventBody::BookDelta`].
    BookDelta = 0,
    /// Corresponds to [`EventBody::Trade`].
    Trade = 1,
    /// Corresponds to [`EventBody::TopOfBook`].
    TopOfBook = 2,
    /// Corresponds to [`EventBody::OrderAck`].
    OrderAck = 3,
    /// Corresponds to [`EventBody::Fill`].
    Fill = 4,
    /// Corresponds to [`EventBody::OrderReject`].
    OrderReject = 5,
    /// Corresponds to [`EventBody::Timer`].
    Timer = 6,
    /// Corresponds to [`EventBody::Heartbeat`].
    Heartbeat = 7,
}

impl fmt::Debug for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BookDelta => write!(f, "EventKind::BookDelta"),
            Self::Trade => write!(f, "EventKind::Trade"),
            Self::TopOfBook => write!(f, "EventKind::TopOfBook"),
            Self::OrderAck => write!(f, "EventKind::OrderAck"),
            Self::Fill => write!(f, "EventKind::Fill"),
            Self::OrderReject => write!(f, "EventKind::OrderReject"),
            Self::Timer => write!(f, "EventKind::Timer"),
            Self::Heartbeat => write!(f, "EventKind::Heartbeat"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;
    use mantis_types::{Lots, OrderId, Side, Ticks};

    use crate::control::{HeartbeatPayload, TimerKind, TimerPayload};
    use crate::execution::{
        FillPayload, OrderAckPayload, OrderRejectPayload, OrderStatus, RejectReason,
    };
    use crate::market::{BookDeltaPayload, TopOfBookPayload, TradePayload, UpdateAction};

    #[test]
    fn event_kind_repr_values() {
        assert_eq!(EventKind::BookDelta as u16, 0);
        assert_eq!(EventKind::Trade as u16, 1);
        assert_eq!(EventKind::TopOfBook as u16, 2);
        assert_eq!(EventKind::OrderAck as u16, 3);
        assert_eq!(EventKind::Fill as u16, 4);
        assert_eq!(EventKind::OrderReject as u16, 5);
        assert_eq!(EventKind::Timer as u16, 6);
        assert_eq!(EventKind::Heartbeat as u16, 7);
    }

    #[test]
    fn event_kind_size_is_2() {
        assert_eq!(mem::size_of::<EventKind>(), 2);
    }

    #[test]
    fn body_kind_book_delta() {
        let body = EventBody::BookDelta(BookDeltaPayload {
            price: Ticks::from_raw(100),
            qty: Lots::from_raw(10),
            side: Side::Bid,
            action: UpdateAction::New,
            depth: 0,
            _pad: [0; 5],
        });
        assert_eq!(body.kind(), EventKind::BookDelta);
    }

    #[test]
    fn body_kind_trade() {
        let body = EventBody::Trade(TradePayload {
            price: Ticks::from_raw(200),
            qty: Lots::from_raw(5),
            aggressor: Side::Ask,
            _pad: [0; 7],
        });
        assert_eq!(body.kind(), EventKind::Trade);
    }

    #[test]
    fn body_kind_top_of_book() {
        let body = EventBody::TopOfBook(TopOfBookPayload {
            bid_price: Ticks::from_raw(99),
            bid_qty: Lots::from_raw(50),
            ask_price: Ticks::from_raw(101),
            ask_qty: Lots::from_raw(30),
        });
        assert_eq!(body.kind(), EventKind::TopOfBook);
    }

    #[test]
    fn body_kind_order_ack() {
        let body = EventBody::OrderAck(OrderAckPayload {
            order_id: OrderId::from_raw(1),
            client_order_id: 2,
            status: OrderStatus::Accepted,
            _pad: [0; 7],
        });
        assert_eq!(body.kind(), EventKind::OrderAck);
    }

    #[test]
    fn body_kind_fill() {
        let body = EventBody::Fill(FillPayload {
            order_id: OrderId::from_raw(7),
            price: Ticks::from_raw(500),
            qty: Lots::from_raw(3),
            side: Side::Bid,
            is_maker: 1,
            _pad: [0; 6],
        });
        assert_eq!(body.kind(), EventKind::Fill);
    }

    #[test]
    fn body_kind_order_reject() {
        let body = EventBody::OrderReject(OrderRejectPayload {
            order_id: OrderId::from_raw(0),
            client_order_id: 55,
            reason: RejectReason::InvalidPrice,
            _pad: [0; 7],
        });
        assert_eq!(body.kind(), EventKind::OrderReject);
    }

    #[test]
    fn body_kind_timer() {
        let body = EventBody::Timer(TimerPayload {
            timer_id: 3,
            kind: TimerKind::Periodic,
            _pad: [0; 3],
        });
        assert_eq!(body.kind(), EventKind::Timer);
    }

    #[test]
    fn body_kind_heartbeat() {
        let body = EventBody::Heartbeat(HeartbeatPayload { counter: 42 });
        assert_eq!(body.kind(), EventKind::Heartbeat);
    }
}
