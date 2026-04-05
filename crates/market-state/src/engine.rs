//! Market-state engine — passive deterministic state machine.

use crate::book::OrderBook;
use crate::state::{InstrumentState, TopOfBook};

/// Venue-agnostic market-state engine.
///
/// Generic over book type `B` and max instrument count `MAX`.
/// Fully stack-allocated with `ArrayBook` — no allocator needed.
#[expect(dead_code, reason = "fields used in future tasks")]
pub struct MarketStateEngine<B: OrderBook, const MAX: usize> {
    pub(crate) instruments: [InstrumentState<B>; MAX],
    pub(crate) active_count: usize,
    pub(crate) stale_timeout_ns: u64,
    pub(crate) tob_changed: bool,
    pub(crate) last_tob: Option<TopOfBook>,
}
