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
            .map(|p| p.qty)
            .unwrap_or(SignedLots::ZERO);

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
                if pos.instrument_id == *inst {
                    if let Some(n) = pos.notional(*mid).checked_add(total) {
                        total = n;
                    }
                }
            }
        }
        total
    }
}
