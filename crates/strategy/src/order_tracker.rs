//! Per-strategy order tracking with fixed-size storage.

use mantis_types::{InstrumentId, Lots, Side, Ticks};

/// Maximum orders tracked per strategy.
pub const MAX_TRACKED_ORDERS: usize = 64;

/// Venue-agnostic order states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderState {
    /// Intent emitted, not yet sent to venue.
    Pending = 0,
    /// Sent to venue, awaiting ack.
    Sending = 1,
    /// Resting on the book (including partially filled).
    Live = 2,
    /// Cancel request sent, awaiting confirmation.
    CancelPending = 3,
    /// Fully filled.
    Filled = 4,
    /// Cancelled (by us or venue).
    Cancelled = 5,
    /// Rejected by venue.
    Rejected = 6,
}

/// A tracked order.
///
/// Partially filled live: `state == Live && filled_qty > 0`.
#[derive(Clone, Copy, Debug)]
pub struct TrackedOrder {
    /// Client-assigned order identifier.
    pub client_order_id: u64,
    /// Exchange-assigned order ID. 0 means not yet acked.
    pub exchange_order_id: u64,
    /// Strategy that owns this order.
    pub strategy_id: u8,
    /// Instrument this order is for.
    pub instrument_id: InstrumentId,
    /// Side of the order.
    pub side: Side,
    /// Price in ticks.
    pub price: Ticks,
    /// Original order quantity.
    pub original_qty: Lots,
    /// Cumulative filled quantity.
    pub filled_qty: Lots,
    /// Current lifecycle state.
    pub state: OrderState,
    /// Wall-clock nanoseconds when the intent was sent.
    pub created_at_ns: u64,
    /// 0 means not yet acked. Latency metrics MUST check > 0.
    pub acked_at_ns: u64,
}

impl TrackedOrder {
    /// Remaining unfilled quantity.
    #[must_use]
    pub fn remaining_qty(&self) -> Lots {
        self.original_qty - self.filled_qty
    }

    /// True if order is active (could still fill or needs cancellation).
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            OrderState::Pending
                | OrderState::Sending
                | OrderState::Live
                | OrderState::CancelPending
        )
    }
}

/// Error when tracker is at capacity.
#[derive(Debug, Clone, Copy)]
pub struct TrackerFullError;

/// Tracks all orders for one strategy. Fixed-size, no heap.
pub struct OrderTracker {
    orders: [Option<TrackedOrder>; MAX_TRACKED_ORDERS],
    active_count: usize,
}

impl OrderTracker {
    /// Create an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            orders: [None; MAX_TRACKED_ORDERS],
            active_count: 0,
        }
    }

    /// Register a new order intent.
    ///
    /// # Errors
    ///
    /// Returns [`TrackerFullError`] if all `MAX_TRACKED_ORDERS` slots are occupied.
    pub fn on_intent_sent(&mut self, order: TrackedOrder) -> Result<(), TrackerFullError> {
        for slot in &mut self.orders {
            if slot.is_none() {
                *slot = Some(order);
                self.active_count += 1;
                return Ok(());
            }
        }
        Err(TrackerFullError)
    }

    /// Order was acknowledged by venue.
    pub fn on_ack(&mut self, client_order_id: u64, acked_at_ns: u64) {
        if let Some(order) = self.find_mut(client_order_id) {
            order.state = OrderState::Live;
            order.acked_at_ns = acked_at_ns;
        }
    }

    /// Order was rejected by venue.
    pub fn on_reject(&mut self, client_order_id: u64) {
        if let Some(order) = self.find_mut(client_order_id) {
            order.state = OrderState::Rejected;
            self.active_count = self.active_count.saturating_sub(1);
        }
    }

    /// Partial or full fill received.
    pub fn on_fill(&mut self, client_order_id: u64, fill_qty: Lots) {
        if let Some(order) = self.find_mut(client_order_id) {
            order.filled_qty += fill_qty;
            if order.filled_qty >= order.original_qty {
                order.state = OrderState::Filled;
                self.active_count = self.active_count.saturating_sub(1);
            }
        }
    }

    /// Cancel was confirmed.
    pub fn on_cancel_ack(&mut self, client_order_id: u64) {
        if let Some(order) = self.find_mut(client_order_id) {
            order.state = OrderState::Cancelled;
            self.active_count = self.active_count.saturating_sub(1);
        }
    }

    /// Lookup order by `client_order_id`.
    #[must_use]
    pub fn get(&self, client_order_id: u64) -> Option<&TrackedOrder> {
        self.orders
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|o| o.client_order_id == client_order_id)
    }

    /// Number of active orders.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active_count
    }

    /// Total open (unfilled) quantity for an instrument + side.
    #[must_use]
    pub fn open_qty(&self, instrument_id: InstrumentId, side: Side) -> Lots {
        let mut total = Lots::ZERO;
        for order in self.orders.iter().filter_map(|s| s.as_ref()) {
            if order.is_active()
                && order.instrument_id == instrument_id
                && order.side == side
            {
                total += order.remaining_qty();
            }
        }
        total
    }

    /// Iterator over active orders.
    pub fn active_orders(&self) -> impl Iterator<Item = &TrackedOrder> {
        self.orders
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|o| o.is_active())
    }

    fn find_mut(&mut self, client_order_id: u64) -> Option<&mut TrackedOrder> {
        self.orders
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|o| o.client_order_id == client_order_id)
    }
}

impl Default for OrderTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mantis_types::{InstrumentId, Lots, Side, Ticks};

    fn make_order(id: u64) -> TrackedOrder {
        TrackedOrder {
            client_order_id: id,
            exchange_order_id: 0,
            strategy_id: 0,
            instrument_id: InstrumentId::from_raw(1),
            side: Side::Bid,
            price: Ticks::from_raw(650),
            original_qty: Lots::from_raw(100),
            filled_qty: Lots::ZERO,
            state: OrderState::Pending,
            created_at_ns: 1000,
            acked_at_ns: 0,
        }
    }

    #[test]
    fn track_and_ack() {
        let mut tracker = OrderTracker::new();
        let order = make_order(1);
        assert!(tracker.on_intent_sent(order).is_ok());
        assert_eq!(tracker.active_count(), 1);

        tracker.on_ack(1, 999);
        let o = tracker.get(1).unwrap();
        assert_eq!(o.state, OrderState::Live);
        assert_eq!(o.acked_at_ns, 999);
    }

    #[test]
    fn fill_updates_qty() {
        let mut tracker = OrderTracker::new();
        tracker.on_intent_sent(make_order(1)).unwrap();
        tracker.on_ack(1, 100);
        tracker.on_fill(1, Lots::from_raw(30));

        let o = tracker.get(1).unwrap();
        assert_eq!(o.filled_qty.to_raw(), 30);
        assert_eq!(o.state, OrderState::Live); // partial fill
        assert_eq!(o.remaining_qty().to_raw(), 70);
    }

    #[test]
    fn full_fill_transitions_to_filled() {
        let mut tracker = OrderTracker::new();
        tracker.on_intent_sent(make_order(1)).unwrap();
        tracker.on_ack(1, 100);
        tracker.on_fill(1, Lots::from_raw(100));

        let o = tracker.get(1).unwrap();
        assert_eq!(o.state, OrderState::Filled);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn cancel_ack() {
        let mut tracker = OrderTracker::new();
        tracker.on_intent_sent(make_order(1)).unwrap();
        tracker.on_ack(1, 100);
        tracker.on_cancel_ack(1);

        let o = tracker.get(1).unwrap();
        assert_eq!(o.state, OrderState::Cancelled);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn reject() {
        let mut tracker = OrderTracker::new();
        tracker.on_intent_sent(make_order(1)).unwrap();
        tracker.on_reject(1);

        let o = tracker.get(1).unwrap();
        assert_eq!(o.state, OrderState::Rejected);
        assert_eq!(tracker.active_count(), 0);
    }

    #[test]
    fn capacity_full_returns_error() {
        let mut tracker = OrderTracker::new();
        for i in 0..MAX_TRACKED_ORDERS {
            tracker.on_intent_sent(make_order(i as u64)).unwrap();
        }
        let result = tracker.on_intent_sent(make_order(999));
        assert!(result.is_err());
    }

    #[test]
    fn open_qty_by_instrument_side() {
        let mut tracker = OrderTracker::new();
        let mut o = make_order(1);
        o.original_qty = Lots::from_raw(100);
        tracker.on_intent_sent(o).unwrap();
        tracker.on_ack(1, 100);

        let mut o2 = make_order(2);
        o2.original_qty = Lots::from_raw(50);
        tracker.on_intent_sent(o2).unwrap();
        tracker.on_ack(2, 100);

        let qty = tracker.open_qty(InstrumentId::from_raw(1), Side::Bid);
        assert_eq!(qty.to_raw(), 150);
    }
}
