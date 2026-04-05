//! Per-instrument state and derived types.

use mantis_types::{InstrumentId, Lots, SeqNum, Side, Ticks, Timestamp};

use crate::book::OrderBook;

/// Per-instrument state managed by the engine.
#[expect(dead_code, reason = "fields accessed in engine impl (Task 4)")]
pub struct InstrumentState<B: OrderBook> {
    pub(crate) book: B,
    pub(crate) prev_bid: Option<Ticks>,
    pub(crate) prev_ask: Option<Ticks>,
    pub(crate) last_trade: Option<TradeInfo>,
    pub(crate) last_event_ts: Timestamp,
    pub(crate) seq: SeqNum,
    pub(crate) snapshot_received: bool,
    pub(crate) in_snapshot: bool,
}

impl<B: OrderBook> Default for InstrumentState<B> {
    fn default() -> Self {
        Self {
            book: B::default(),
            prev_bid: None,
            prev_ask: None,
            last_trade: None,
            last_event_ts: Timestamp::from_nanos(0),
            seq: SeqNum::ZERO,
            snapshot_received: false,
            in_snapshot: false,
        }
    }
}

/// Last trade information.
#[derive(Clone, Copy, Debug)]
pub struct TradeInfo {
    /// Execution price in ticks.
    pub price: Ticks,
    /// Executed quantity in lots.
    pub qty: Lots,
    /// Side of the aggressing order.
    pub side: Side,
    /// Timestamp of the trade.
    pub ts: Timestamp,
}

/// Top-of-book state emitted on BBO price change.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TopOfBook {
    /// Best bid price in ticks.
    pub bid_price: Ticks,
    /// Best bid quantity in lots.
    pub bid_qty: Lots,
    /// Best ask price in ticks.
    pub ask_price: Ticks,
    /// Best ask quantity in lots.
    pub ask_qty: Lots,
    /// Volume-weighted micro price: `(bid × ask_sz + ask × bid_sz) / (bid_sz + ask_sz)`.
    pub micro_price: Ticks,
    /// Spread in ticks: `ask_price - bid_price`.
    pub spread: Ticks,
    /// Instrument this top-of-book corresponds to.
    pub instrument_id: InstrumentId,
    /// Timestamp of the triggering event.
    pub ts: Timestamp,
}
