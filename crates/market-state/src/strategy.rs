//! Strategy trait and order intent types.

use mantis_types::{InstrumentId, Lots, Side, Ticks};

use crate::book::OrderBook;
use crate::engine::MarketStateEngine;
use crate::state::TopOfBook;

/// Maximum order intents per tick. Fixed-size to avoid allocation.
pub const MAX_INTENTS_PER_TICK: usize = 8;

/// Order intent emitted by strategy for the execution engine.
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct OrderIntent {
    /// Instrument this intent is for.
    pub instrument_id: InstrumentId,
    /// Side of the order.
    pub side: Side,
    /// Price in ticks.
    pub price: Ticks,
    /// Quantity in lots.
    pub qty: Lots,
    /// Action to perform.
    pub action: OrderAction,
    /// Client-assigned order identifier.
    pub client_order_id: u64,
}

/// Order action type.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderAction {
    /// Post a new order.
    #[default]
    Post = 0,
    /// Cancel an existing order.
    Cancel = 1,
    /// Amend an existing order.
    Amend = 2,
}

/// Strategy trait — implemented by the binary.
///
/// `on_tick` is called inline on the hot thread when a BBO price change
/// is detected. Strategy has direct `&mut engine` access to all books and
/// derived metrics (queries may update BBO caches).
pub trait Strategy<B: OrderBook, const MAX: usize> {
    /// Called when a BBO price change is detected on any instrument.
    ///
    /// `tob` identifies which instrument changed and its new BBO.
    /// Returns the number of order intents written to `intents`.
    fn on_tick(
        &mut self,
        engine: &mut MarketStateEngine<B, MAX>,
        tob: &TopOfBook,
        intents: &mut [OrderIntent; MAX_INTENTS_PER_TICK],
    ) -> usize;
}
