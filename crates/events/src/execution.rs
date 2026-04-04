//! Execution event payloads and supporting enums.

use core::fmt;

use mantis_types::{Lots, OrderId, Side, Ticks};

/// The current lifecycle state of an order.
///
/// Note: `Filled` is intentionally absent. Fills are represented by
/// [`FillPayload`] as a separate event variant, not as an order status,
/// because fills carry additional data (price, qty, side, maker flag).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OrderStatus {
    /// The order has been accepted by the venue.
    Accepted = 0,
    /// The order has been cancelled.
    Cancelled = 1,
    /// The order has expired (e.g. GTD or IOC with no fill).
    Expired = 2,
}

impl fmt::Debug for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => write!(f, "OrderStatus::Accepted"),
            Self::Cancelled => write!(f, "OrderStatus::Cancelled"),
            Self::Expired => write!(f, "OrderStatus::Expired"),
        }
    }
}

/// The reason an order was rejected by the venue or risk layer.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RejectReason {
    /// Account does not have sufficient funds to cover the order.
    InsufficientFunds = 0,
    /// The submitted price is outside valid bounds.
    InvalidPrice = 1,
    /// The submitted quantity is outside valid bounds.
    InvalidQty = 2,
    /// An order with this ID already exists.
    DuplicateOrderId = 3,
    /// The order would breach a configured risk limit.
    RiskLimitBreached = 4,
    /// The venue returned an unspecified error.
    VenueError = 5,
    /// The reject reason could not be mapped to a known variant.
    Unknown = 255,
}

impl fmt::Debug for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InsufficientFunds => write!(f, "RejectReason::InsufficientFunds"),
            Self::InvalidPrice => write!(f, "RejectReason::InvalidPrice"),
            Self::InvalidQty => write!(f, "RejectReason::InvalidQty"),
            Self::DuplicateOrderId => write!(f, "RejectReason::DuplicateOrderId"),
            Self::RiskLimitBreached => write!(f, "RejectReason::RiskLimitBreached"),
            Self::VenueError => write!(f, "RejectReason::VenueError"),
            Self::Unknown => write!(f, "RejectReason::Unknown"),
        }
    }
}

/// Acknowledgement that a submitted order has been accepted or updated.
#[derive(Clone, Copy)]
#[repr(C)]
#[expect(clippy::pub_underscore_fields, reason = "repr(C) padding field")]
pub struct OrderAckPayload {
    /// Venue-assigned order identifier.
    pub order_id: OrderId,
    /// Client-assigned order identifier (echoed back by the venue).
    pub client_order_id: u64,
    /// New lifecycle status of the order.
    pub status: OrderStatus,
    /// Reserved padding — must be zero.
    pub _pad: [u8; 7],
}

/// A single partial or full fill.
///
/// Fills are always separate events rather than order-status updates because
/// they carry price, quantity, side, and maker/taker information.
#[derive(Clone, Copy)]
#[repr(C)]
#[expect(clippy::pub_underscore_fields, reason = "repr(C) padding field")]
pub struct FillPayload {
    /// Venue-assigned order identifier of the filled order.
    pub order_id: OrderId,
    /// Execution price in ticks.
    pub price: Ticks,
    /// Executed quantity in lots.
    pub qty: Lots,
    /// Side of the filled order.
    pub side: Side,
    /// `1` if this order was the passive (maker) side, `0` if taker.
    ///
    /// Stored as `u8` rather than `bool` for predictable `repr(C)` layout.
    pub is_maker: u8,
    /// Reserved padding — must be zero.
    pub _pad: [u8; 6],
}

/// Notification that an order submission was rejected.
#[derive(Clone, Copy)]
#[repr(C)]
#[expect(clippy::pub_underscore_fields, reason = "repr(C) padding field")]
pub struct OrderRejectPayload {
    /// Venue-assigned order identifier (may be zero if rejected pre-submission).
    pub order_id: OrderId,
    /// Client-assigned order identifier.
    pub client_order_id: u64,
    /// Reason the order was rejected.
    pub reason: RejectReason,
    /// Reserved padding — must be zero.
    pub _pad: [u8; 7],
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn order_status_repr_values() {
        assert_eq!(OrderStatus::Accepted as u8, 0);
        assert_eq!(OrderStatus::Cancelled as u8, 1);
        assert_eq!(OrderStatus::Expired as u8, 2);
    }

    #[test]
    fn order_status_size_is_1() {
        assert_eq!(mem::size_of::<OrderStatus>(), 1);
    }

    #[test]
    fn reject_reason_repr_values() {
        assert_eq!(RejectReason::InsufficientFunds as u8, 0);
        assert_eq!(RejectReason::InvalidPrice as u8, 1);
        assert_eq!(RejectReason::InvalidQty as u8, 2);
        assert_eq!(RejectReason::DuplicateOrderId as u8, 3);
        assert_eq!(RejectReason::RiskLimitBreached as u8, 4);
        assert_eq!(RejectReason::VenueError as u8, 5);
        assert_eq!(RejectReason::Unknown as u8, 255);
    }

    #[test]
    fn reject_reason_size_is_1() {
        assert_eq!(mem::size_of::<RejectReason>(), 1);
    }

    #[test]
    fn order_ack_payload_size_is_24() {
        assert_eq!(mem::size_of::<OrderAckPayload>(), 24);
    }

    #[test]
    fn fill_payload_size_is_32() {
        assert_eq!(mem::size_of::<FillPayload>(), 32);
    }

    #[test]
    fn order_reject_payload_size_is_24() {
        assert_eq!(mem::size_of::<OrderRejectPayload>(), 24);
    }

    #[test]
    fn order_ack_roundtrip() {
        let p = OrderAckPayload {
            order_id: OrderId::from_raw(42),
            client_order_id: 99,
            status: OrderStatus::Accepted,
            _pad: [0; 7],
        };
        assert_eq!(p.order_id, OrderId::from_raw(42));
        assert_eq!(p.client_order_id, 99);
        assert_eq!(p.status, OrderStatus::Accepted);
    }

    #[test]
    fn fill_roundtrip() {
        let p = FillPayload {
            order_id: OrderId::from_raw(7),
            price: Ticks::from_raw(500),
            qty: Lots::from_raw(3),
            side: Side::Bid,
            is_maker: 1,
            _pad: [0; 6],
        };
        assert_eq!(p.order_id, OrderId::from_raw(7));
        assert_eq!(p.price, Ticks::from_raw(500));
        assert_eq!(p.qty, Lots::from_raw(3));
        assert_eq!(p.side, Side::Bid);
        assert_eq!(p.is_maker, 1);
    }

    #[test]
    fn order_reject_roundtrip() {
        let p = OrderRejectPayload {
            order_id: OrderId::from_raw(0),
            client_order_id: 55,
            reason: RejectReason::InvalidPrice,
            _pad: [0; 7],
        };
        assert_eq!(p.client_order_id, 55);
        assert_eq!(p.reason, RejectReason::InvalidPrice);
    }
}
