//! Exposure computation composing position + open orders.

use mantis_fixed::FixedI64;
use mantis_types::{InstrumentId, Side, SignedLots};

use crate::order_tracker::OrderTracker;
use crate::position::Position;

/// Read-only view composing position + open orders for risk checks.
///
/// The Risk Gate creates this transiently to evaluate an intent.
/// It does NOT include staged (not-yet-submitted) intents — the gate
/// tracks a running tally separately during intra-batch processing.
pub struct ExposureView<'a> {
    positions: &'a [Position],
    orders: &'a OrderTracker,
}

impl<'a> ExposureView<'a> {
    /// Create a new exposure view.
    #[must_use]
    pub fn new(positions: &'a [Position], orders: &'a OrderTracker) -> Self {
        Self { positions, orders }
    }

    /// Worst-case position if all open orders on this side fill.
    #[must_use]
    pub fn worst_case_qty(&self, instrument_id: InstrumentId, side: Side) -> SignedLots {
        let current = self
            .positions
            .iter()
            .find(|p| p.instrument_id == instrument_id)
            .map_or(SignedLots::ZERO, |p| p.qty);

        let open = self.orders.open_qty(instrument_id, side);
        let delta = match side {
            Side::Bid => SignedLots::from(open),
            Side::Ask => -SignedLots::from(open),
        };

        current + delta
    }

    /// Total notional at risk across all instruments.
    #[must_use]
    pub fn total_notional_at_risk(&self, mid_prices: &[(InstrumentId, FixedI64<6>)]) -> FixedI64<6> {
        let mut total = FixedI64::<6>::ZERO;
        for (inst, mid) in mid_prices {
            for pos in self.positions {
                if pos.instrument_id == *inst
                    && let Some(n) = pos.notional(*mid).checked_add(total)
                {
                    total = n;
                }
            }
        }
        total
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code — panics are acceptable")]
#[expect(clippy::expect_used, reason = "test code — panics are acceptable")]
mod tests {
    use super::*;
    use crate::order_tracker::{OrderState, OrderTracker, TrackedOrder};
    use crate::position::Position;
    use mantis_types::{InstrumentId, Lots, Side, Ticks};

    fn inst(id: u32) -> InstrumentId {
        InstrumentId::from_raw(id)
    }

    fn mid(p: i64) -> FixedI64<6> {
        FixedI64::from_int(p).expect("test price fits")
    }

    fn make_order(id: u64, instrument_id: InstrumentId, side: Side, qty: i64) -> TrackedOrder {
        TrackedOrder {
            client_order_id: id,
            exchange_order_id: 0,
            strategy_id: 0,
            instrument_id,
            side,
            price: Ticks::from_raw(100),
            original_qty: Lots::from_raw(qty),
            filled_qty: Lots::ZERO,
            state: OrderState::Live,
            created_at_ns: 1000,
            acked_at_ns: 2000,
        }
    }

    #[test]
    fn worst_case_qty_no_position_no_orders() {
        let positions: &[Position] = &[];
        let orders = OrderTracker::new();
        let view = ExposureView::new(positions, &orders);

        let wc = view.worst_case_qty(inst(1), Side::Bid);
        assert_eq!(wc, SignedLots::ZERO);
    }

    #[test]
    fn worst_case_qty_bid_adds_open_orders() {
        let mut pos = Position::new(inst(1));
        pos.qty = SignedLots::from_raw(10);
        let positions = [pos];

        let mut orders = OrderTracker::new();
        orders
            .on_intent_sent(make_order(1, inst(1), Side::Bid, 5))
            .unwrap();

        let view = ExposureView::new(&positions, &orders);
        let wc = view.worst_case_qty(inst(1), Side::Bid);
        // current 10 + open bid 5 = 15
        assert_eq!(wc, SignedLots::from_raw(15));
    }

    #[test]
    fn worst_case_qty_ask_subtracts_open_orders() {
        let mut pos = Position::new(inst(1));
        pos.qty = SignedLots::from_raw(10);
        let positions = [pos];

        let mut orders = OrderTracker::new();
        orders
            .on_intent_sent(make_order(1, inst(1), Side::Ask, 3))
            .unwrap();

        let view = ExposureView::new(&positions, &orders);
        let wc = view.worst_case_qty(inst(1), Side::Ask);
        // current 10 - open ask 3 = 7
        assert_eq!(wc, SignedLots::from_raw(7));
    }

    #[test]
    fn total_notional_at_risk_single_instrument() {
        let mut pos = Position::new(inst(1));
        pos.qty = SignedLots::from_raw(10);
        let positions = [pos];

        let orders = OrderTracker::new();
        let view = ExposureView::new(&positions, &orders);

        // notional = |10| * 50 = 500
        let mid_prices = [(inst(1), mid(50))];
        let total = view.total_notional_at_risk(&mid_prices);
        assert_eq!(total, mid(500));
    }

    #[test]
    fn total_notional_at_risk_unmatched_instrument_is_zero() {
        let mut pos = Position::new(inst(2));
        pos.qty = SignedLots::from_raw(10);
        let positions = [pos];

        let orders = OrderTracker::new();
        let view = ExposureView::new(&positions, &orders);

        // mid_prices references inst(1) but position is on inst(2)
        let mid_prices = [(inst(1), mid(50))];
        let total = view.total_notional_at_risk(&mid_prices);
        assert_eq!(total, FixedI64::ZERO);
    }
}
