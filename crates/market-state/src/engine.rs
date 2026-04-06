//! Market-state engine — passive deterministic state machine.

use mantis_events::{EventBody, EventFlags, HotEvent, UpdateAction};
use mantis_types::{InstrumentId, Side, Ticks, Timestamp};

use crate::book::OrderBook;
use crate::state::{InstrumentState, TopOfBook, TradeInfo};

/// Venue-agnostic market-state engine.
///
/// Generic over book type `B` and max instrument count `MAX`.
/// Fully stack-allocated with `ArrayBook` — no allocator needed.
pub struct MarketStateEngine<B: OrderBook, const MAX: usize> {
    pub(crate) instruments: [InstrumentState<B>; MAX],
    pub(crate) active_count: usize,
    pub(crate) stale_timeout_ns: u64,
    pub(crate) tob_changed: bool,
    pub(crate) last_tob: Option<TopOfBook>,
}

impl<B: OrderBook, const MAX: usize> MarketStateEngine<B, MAX> {
    /// Create a new engine with `active_count` instruments.
    ///
    /// Instruments use IDs `1..=active_count` (`InstrumentId::NONE` = 0 is reserved).
    ///
    /// # Panics
    ///
    /// Panics if `active_count > MAX`.
    #[must_use]
    pub fn new(active_count: usize, stale_timeout_ns: u64) -> Self {
        assert!(active_count <= MAX, "active_count exceeds MAX");
        let instruments = core::array::from_fn(|_| InstrumentState::default());
        Self {
            instruments,
            active_count,
            stale_timeout_ns,
            tob_changed: false,
            last_tob: None,
        }
    }

    /// Process one event. Updates book state, detects BBO price changes.
    ///
    /// Snapshot state machine:
    /// - `IS_SNAPSHOT` on first delta in batch: clear book, enter snapshot mode
    /// - `LAST_IN_BATCH` while in snapshot: mark snapshot complete
    /// - Only emit `TopOfBook` when `snapshot_received && !in_snapshot`
    pub fn process(&mut self, event: &HotEvent) {
        let inst = event.header.instrument_id;
        let flags = event.header.flags;

        match event.body {
            EventBody::BookDelta(delta) => {
                let raw = inst.to_raw() as usize;
                if raw == 0 || raw > self.active_count {
                    return;
                }
                let slot = raw - 1;
                let state = &mut self.instruments[slot];
                state.last_event_ts = event.header.recv_ts;
                state.seq = event.header.seq;

                // Snapshot state machine: IS_SNAPSHOT on first delta clears book
                if flags.contains(EventFlags::IS_SNAPSHOT) {
                    state.book.clear();
                    state.in_snapshot = true;
                    state.snapshot_received = false;
                }

                // Apply the delta
                state
                    .book
                    .apply_delta(delta.price, delta.qty, delta.side, delta.action);

                // LAST_IN_BATCH finalizes snapshot or triggers BBO check
                if flags.contains(EventFlags::LAST_IN_BATCH) {
                    if state.in_snapshot {
                        state.in_snapshot = false;
                        state.snapshot_received = true;
                    }
                    if state.snapshot_received
                        && let Some(tob) = self.check_bbo_price_change(slot)
                    {
                        self.last_tob = Some(tob);
                        self.tob_changed = true;
                    }
                }
            }
            EventBody::Trade(trade) => {
                let raw = inst.to_raw() as usize;
                if raw == 0 || raw > self.active_count {
                    return;
                }
                let slot = raw - 1;
                let state = &mut self.instruments[slot];
                state.last_event_ts = event.header.recv_ts;
                state.last_trade = Some(TradeInfo {
                    price: trade.price,
                    qty: trade.qty,
                    side: trade.aggressor,
                    ts: event.header.recv_ts,
                });
            }
            EventBody::TopOfBook(tob_payload) => {
                let raw = inst.to_raw() as usize;
                if raw == 0 || raw > self.active_count {
                    return;
                }
                let slot = raw - 1;
                let state = &mut self.instruments[slot];
                state.last_event_ts = event.header.recv_ts;
                state.snapshot_received = true;

                // Apply as single-level synthetic book
                state.book.clear();
                state.book.apply_delta(
                    tob_payload.bid_price,
                    tob_payload.bid_qty,
                    Side::Bid,
                    UpdateAction::New,
                );
                state.book.apply_delta(
                    tob_payload.ask_price,
                    tob_payload.ask_qty,
                    Side::Ask,
                    UpdateAction::New,
                );

                // TopOfBook events emit immediately
                if let Some(tob) = self.check_bbo_price_change(slot) {
                    self.last_tob = Some(tob);
                    self.tob_changed = true;
                }
            }
            // Timer, Heartbeat, OrderAck, Fill, OrderReject — not processed by engine
            _ => {}
        }
    }

    /// Micro price — volume-weighted fair value between best bid and ask.
    ///
    /// Returns `None` if the instrument ID is invalid or either side is empty.
    pub fn micro_price(&mut self, inst: InstrumentId) -> Option<Ticks> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        let state = &mut self.instruments[slot - 1];
        let (bp, bq) = state.book.best_bid()?;
        let (ap, aq) = state.book.best_ask()?;
        let total = bq.to_raw() + aq.to_raw();
        if total == 0 {
            return None;
        }
        Some(Ticks::from_raw(
            (bp.to_raw() * aq.to_raw() + ap.to_raw() * bq.to_raw()) / total,
        ))
    }

    /// Book imbalance at top `levels` levels.
    ///
    /// Returns a value in `[-1.0, +1.0]`: positive = bid-heavy, negative = ask-heavy.
    /// Returns `None` if the instrument ID is invalid or both sides are empty.
    #[expect(
        clippy::cast_precision_loss,
        reason = "quantity values fit comfortably in f64 mantissa at market scales"
    )]
    pub fn book_imbalance(&mut self, inst: InstrumentId, levels: usize) -> Option<f64> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        let state = &mut self.instruments[slot - 1];
        let bd = state.book.total_depth(Side::Bid, levels).to_raw() as f64;
        let ad = state.book.total_depth(Side::Ask, levels).to_raw() as f64;
        if bd + ad == 0.0 {
            return None;
        }
        Some((bd - ad) / (bd + ad))
    }

    /// Spread in ticks: `ask_price - bid_price`.
    ///
    /// Returns `None` if the instrument ID is invalid or either side is empty.
    pub fn spread(&mut self, inst: InstrumentId) -> Option<Ticks> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        let state = &mut self.instruments[slot - 1];
        let (bp, _) = state.book.best_bid()?;
        let (ap, _) = state.book.best_ask()?;
        Some(Ticks::from_raw(ap.to_raw() - bp.to_raw()))
    }

    /// Last trade info for an instrument, or `None` if no trade seen yet.
    pub fn last_trade(&self, inst: InstrumentId) -> Option<&TradeInfo> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        self.instruments[slot - 1].last_trade.as_ref()
    }

    /// Returns `true` if no event has been received within `stale_timeout_ns` nanoseconds.
    ///
    /// An instrument with no events at all (`last_event_ts == 0`) is always considered stale.
    /// An invalid instrument ID is also considered stale.
    pub fn is_stale(&self, inst: InstrumentId, now: Timestamp) -> bool {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return true;
        }
        let last = self.instruments[slot - 1].last_event_ts.as_nanos();
        if last == 0 {
            return true;
        }
        now.as_nanos().saturating_sub(last) > self.stale_timeout_ns
    }

    /// Returns `true` if the initial snapshot has been received for this instrument.
    ///
    /// Returns `false` for invalid instrument IDs.
    pub fn is_ready(&self, inst: InstrumentId) -> bool {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return false;
        }
        self.instruments[slot - 1].snapshot_received
    }

    /// Direct access to the underlying order book.
    ///
    /// Returns `None` if the instrument ID is invalid.
    pub fn book(&mut self, inst: InstrumentId) -> Option<&mut B> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        Some(&mut self.instruments[slot - 1].book)
    }

    /// Take the last `TopOfBook` if a BBO price change was detected.
    ///
    /// Clears the flag so subsequent calls return `None` until the next change.
    pub fn take_tob(&mut self) -> Option<TopOfBook> {
        if self.tob_changed {
            self.tob_changed = false;
            self.last_tob
        } else {
            None
        }
    }

    /// Check if the BBO price changed for the given instrument slot.
    ///
    /// Returns `Some(TopOfBook)` only when both bid and ask exist and at
    /// least one price differs from the previous observation.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "slot < active_count <= MAX which is bounded by u32::MAX in new()"
    )]
    fn check_bbo_price_change(&mut self, slot: usize) -> Option<TopOfBook> {
        let state = &mut self.instruments[slot];
        let bid = state.book.best_bid();
        let ask = state.book.best_ask();
        let bid_price = bid.map(|(p, _)| p);
        let ask_price = ask.map(|(p, _)| p);

        if bid_price == state.prev_bid && ask_price == state.prev_ask {
            return None;
        }

        state.prev_bid = bid_price;
        state.prev_ask = ask_price;

        match (bid, ask) {
            (Some((bp, bq)), Some((ap, aq))) => {
                let total = bq.to_raw() + aq.to_raw();
                let micro = if total > 0 {
                    Ticks::from_raw((bp.to_raw() * aq.to_raw() + ap.to_raw() * bq.to_raw()) / total)
                } else {
                    Ticks::from_raw(i64::midpoint(bp.to_raw(), ap.to_raw()))
                };
                Some(TopOfBook {
                    bid_price: bp,
                    bid_qty: bq,
                    ask_price: ap,
                    ask_qty: aq,
                    micro_price: micro,
                    spread: Ticks::from_raw(ap.to_raw() - bp.to_raw()),
                    instrument_id: InstrumentId::from_raw((slot + 1) as u32),
                    ts: state.last_event_ts,
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::ArrayBook;
    use mantis_events::{BookDeltaPayload, EventFlags, HotEvent, TopOfBookPayload, TradePayload};
    use mantis_types::{InstrumentId, Lots, SeqNum, Side, SourceId, Ticks, Timestamp};

    fn make_delta(inst: u32, price: i64, qty: i64, side: Side, flags: EventFlags) -> HotEvent {
        HotEvent::book_delta(
            Timestamp::from_nanos(1000),
            SeqNum::from_raw(1),
            InstrumentId::from_raw(inst),
            SourceId::from_raw(1),
            flags,
            BookDeltaPayload {
                price: Ticks::from_raw(price),
                qty: Lots::from_raw(qty),
                side,
                action: UpdateAction::New,
                depth: 0,
                _pad: [0; 5],
            },
        )
    }

    #[expect(clippy::too_many_arguments, reason = "test helper — all params are semantically distinct")]
    fn make_delta_ts(
        inst: u32,
        price: i64,
        qty: i64,
        side: Side,
        flags: EventFlags,
        ts_ns: u64,
    ) -> HotEvent {
        HotEvent::book_delta(
            Timestamp::from_nanos(ts_ns),
            SeqNum::from_raw(1),
            InstrumentId::from_raw(inst),
            SourceId::from_raw(1),
            flags,
            BookDeltaPayload {
                price: Ticks::from_raw(price),
                qty: Lots::from_raw(qty),
                side,
                action: UpdateAction::New,
                depth: 0,
                _pad: [0; 5],
            },
        )
    }

    fn make_trade(inst: u32, price: i64, qty: i64, aggressor: Side, ts_ns: u64) -> HotEvent {
        HotEvent::trade(
            Timestamp::from_nanos(ts_ns),
            SeqNum::from_raw(1),
            InstrumentId::from_raw(inst),
            SourceId::from_raw(1),
            EventFlags::EMPTY,
            TradePayload {
                price: Ticks::from_raw(price),
                qty: Lots::from_raw(qty),
                aggressor,
                _pad: [0; 7],
            },
        )
    }

    #[expect(clippy::too_many_arguments, reason = "test helper — all params are semantically distinct")]
    fn make_top_of_book(
        inst: u32,
        bid_price: i64,
        bid_qty: i64,
        ask_price: i64,
        ask_qty: i64,
        ts_ns: u64,
    ) -> HotEvent {
        HotEvent::top_of_book(
            Timestamp::from_nanos(ts_ns),
            SeqNum::from_raw(1),
            InstrumentId::from_raw(inst),
            SourceId::from_raw(1),
            EventFlags::EMPTY,
            TopOfBookPayload {
                bid_price: Ticks::from_raw(bid_price),
                bid_qty: Lots::from_raw(bid_qty),
                ask_price: Ticks::from_raw(ask_price),
                ask_qty: Lots::from_raw(ask_qty),
            },
        )
    }

    /// Helper: build a two-sided book for instrument 1 via snapshot deltas.
    ///
    /// Prices must be in `[0, 99]` (valid indices for `ArrayBook<100>`).
    fn setup_book(
        engine: &mut MarketStateEngine<ArrayBook<100>, 4>,
        bid: i64,
        bid_qty: i64,
        ask: i64,
        ask_qty: i64,
    ) {
        let snap1 = make_delta(1, bid, bid_qty, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, ask, ask_qty, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
    }

    #[test]
    fn new_engine_has_no_tob() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 1_000_000_000);
        assert!(engine.take_tob().is_none());
    }

    #[test]
    fn process_delta_no_tob_before_snapshot() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 1_000_000_000);
        let ev = make_delta(1, 45, 100, Side::Bid, EventFlags::LAST_IN_BATCH);
        engine.process(&ev);
        assert!(engine.take_tob().is_none()); // no snapshot yet
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn process_snapshot_then_delta_emits_tob() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 1_000_000_000);
        // Snapshot: IS_SNAPSHOT on first, LAST_IN_BATCH on last
        let snap1 = make_delta(1, 45, 100, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, 55, 200, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
        let tob = engine
            .take_tob()
            .expect("expected TopOfBook after snapshot");
        assert_eq!(tob.bid_price, Ticks::from_raw(45));
        assert_eq!(tob.ask_price, Ticks::from_raw(55));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn micro_price_between_bid_ask() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 1_000_000_000);
        let snap1 = make_delta(1, 45, 100, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, 55, 200, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
        let mp = engine
            .micro_price(InstrumentId::from_raw(1))
            .expect("expected micro_price");
        assert!(mp.to_raw() >= 45 && mp.to_raw() <= 55);
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn book_imbalance_range() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 1_000_000_000);
        let snap1 = make_delta(1, 45, 1000, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, 55, 100, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
        let imb = engine
            .book_imbalance(InstrumentId::from_raw(1), 5)
            .expect("expected book_imbalance");
        assert!(imb > 0.0); // bid heavy
        assert!(imb <= 1.0);
    }

    #[test]
    fn is_stale_after_timeout() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 100);
        let snap = make_delta(
            1,
            45,
            100,
            Side::Bid,
            EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH),
        );
        engine.process(&snap);
        // Within timeout: ts=1000, now=1050, delta=50 <= 100
        assert!(!engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1050)));
        // Beyond timeout: ts=1000, now=1200, delta=200 > 100
        assert!(engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1200)));
    }

    // -----------------------------------------------------------------------
    // spread() — catches: `- with +`, `> with >=`, missing-BBO None
    // -----------------------------------------------------------------------

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn spread_returns_ask_minus_bid() {
        // bid=40, ask=45  →  spread must be exactly 5, not 85 (+ mutation)
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 10, 45, 10);
        let s = engine
            .spread(InstrumentId::from_raw(1))
            .expect("spread should be Some");
        assert_eq!(s, Ticks::from_raw(5), "spread must equal ask - bid = 5");
    }

    #[test]
    fn spread_returns_none_for_invalid_instrument() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        // instrument 0 is NONE, instrument 5 is out of range
        assert!(engine.spread(InstrumentId::NONE).is_none());
        assert!(engine.spread(InstrumentId::from_raw(5)).is_none());
    }

    #[test]
    fn spread_returns_none_with_no_bbo() {
        // Empty book → spread must be None (both sides absent)
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(engine.spread(InstrumentId::from_raw(1)).is_none());
    }

    #[test]
    fn spread_returns_none_with_only_bid() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        // Provide only a bid side via snapshot
        let snap = make_delta(
            1,
            40,
            10,
            Side::Bid,
            EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH),
        );
        engine.process(&snap);
        assert!(engine.spread(InstrumentId::from_raw(1)).is_none());
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn spread_minimum_one_tick() {
        // bid=50, ask=51  →  spread = 1
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 50, 50, 51, 50);
        let s = engine
            .spread(InstrumentId::from_raw(1))
            .expect("spread Some");
        assert_eq!(s, Ticks::from_raw(1));
    }

    // -----------------------------------------------------------------------
    // last_trade() — catches: None when no trade, wrong instrument, field values
    // -----------------------------------------------------------------------

    #[test]
    fn last_trade_returns_none_before_any_trade() {
        let engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(engine.last_trade(InstrumentId::from_raw(1)).is_none());
    }

    #[test]
    fn last_trade_returns_none_for_invalid_instrument() {
        let engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(engine.last_trade(InstrumentId::NONE).is_none());
        assert!(engine.last_trade(InstrumentId::from_raw(5)).is_none());
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn last_trade_returns_correct_fields() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let ev = make_trade(1, 999, 42, Side::Ask, 5_000);
        engine.process(&ev);
        let trade = engine
            .last_trade(InstrumentId::from_raw(1))
            .expect("trade should be Some");
        assert_eq!(trade.price, Ticks::from_raw(999));
        assert_eq!(trade.qty, Lots::from_raw(42));
        assert_eq!(trade.side, Side::Ask);
        assert_eq!(trade.ts, Timestamp::from_nanos(5_000));
    }

    #[test]
    fn last_trade_is_none_for_different_instrument() {
        // Trade on instrument 1 must not appear on instrument 2
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let ev = make_trade(1, 100, 10, Side::Bid, 1_000);
        engine.process(&ev);
        assert!(engine.last_trade(InstrumentId::from_raw(2)).is_none());
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn last_trade_updated_by_most_recent_event() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        engine.process(&make_trade(1, 100, 10, Side::Bid, 1_000));
        engine.process(&make_trade(1, 200, 20, Side::Ask, 2_000));
        let trade = engine
            .last_trade(InstrumentId::from_raw(1))
            .expect("trade Some");
        // Must reflect the second (most recent) trade
        assert_eq!(trade.price, Ticks::from_raw(200));
        assert_eq!(trade.qty, Lots::from_raw(20));
        assert_eq!(trade.side, Side::Ask);
    }

    // -----------------------------------------------------------------------
    // is_stale() — catches: `|| with &&`, `> with >=`, boundary exactness
    // -----------------------------------------------------------------------

    #[test]
    fn is_stale_true_for_invalid_instrument() {
        let engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 100);
        assert!(engine.is_stale(InstrumentId::NONE, Timestamp::from_nanos(0)));
        assert!(engine.is_stale(InstrumentId::from_raw(5), Timestamp::from_nanos(0)));
    }

    #[test]
    fn is_stale_true_when_no_events_received() {
        // last_event_ts == 0 → always stale regardless of now
        let engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000);
        assert!(engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(0)));
        assert!(engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(999)));
    }

    #[test]
    fn is_stale_false_within_timeout() {
        // ts=1000, timeout=100  →  now=1100 means delta=100, NOT stale (> not >=)
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 100);
        let ev = make_delta_ts(
            1,
            50,
            10,
            Side::Bid,
            EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH),
            1000,
        );
        engine.process(&ev);
        // delta == timeout exactly: should NOT be stale (> not >=)
        assert!(
            !engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1100)),
            "delta == timeout should not be stale"
        );
        // delta == timeout - 1: also not stale
        assert!(!engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1099)));
    }

    #[test]
    fn is_stale_true_just_beyond_timeout() {
        // ts=1000, timeout=100  →  now=1101 means delta=101 > 100: stale
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 100);
        let ev = make_delta_ts(
            1,
            50,
            10,
            Side::Bid,
            EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH),
            1000,
        );
        engine.process(&ev);
        assert!(engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1101)));
    }

    #[test]
    fn is_stale_fresh_and_stale_instruments_are_independent() {
        // instrument 1 has events, instrument 2 does not →
        // || vs && mutation would flip the outcome for instrument 2
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 100);
        let ev = make_delta_ts(
            1,
            50,
            10,
            Side::Bid,
            EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH),
            1000,
        );
        engine.process(&ev);
        let now = Timestamp::from_nanos(1050);
        assert!(!engine.is_stale(InstrumentId::from_raw(1), now));
        // instrument 2 never received events → always stale
        assert!(engine.is_stale(InstrumentId::from_raw(2), now));
    }

    // -----------------------------------------------------------------------
    // is_ready() — catches: unconditional `return true` mutation
    // -----------------------------------------------------------------------

    #[test]
    fn is_ready_false_before_snapshot() {
        let engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        // No events at all — must be false, not true
        assert!(!engine.is_ready(InstrumentId::from_raw(1)));
    }

    #[test]
    fn is_ready_false_for_invalid_instrument() {
        let engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(!engine.is_ready(InstrumentId::NONE));
        assert!(!engine.is_ready(InstrumentId::from_raw(5)));
    }

    #[test]
    fn is_ready_true_after_snapshot_completed() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 10, 50, 10);
        assert!(engine.is_ready(InstrumentId::from_raw(1)));
    }

    #[test]
    fn is_ready_false_mid_snapshot() {
        // IS_SNAPSHOT sent but LAST_IN_BATCH not yet seen → not ready
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let snap_start = make_delta(1, 100, 10, Side::Bid, EventFlags::IS_SNAPSHOT);
        engine.process(&snap_start);
        assert!(!engine.is_ready(InstrumentId::from_raw(1)));
    }

    #[test]
    fn is_ready_true_after_top_of_book_event() {
        // TopOfBook event marks snapshot_received = true immediately
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let tob_ev = make_top_of_book(1, 100, 10, 110, 10, 1_000);
        engine.process(&tob_ev);
        assert!(engine.is_ready(InstrumentId::from_raw(1)));
    }

    // -----------------------------------------------------------------------
    // micro_price() — catches: wrong operator in weighted formula
    // -----------------------------------------------------------------------

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn micro_price_exact_value_equal_sizes() {
        // bid=40 qty=100, ask=60 qty=100 → equal sizes → micro = (40+60)/2 = 50
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 100, 60, 100);
        let mp = engine
            .micro_price(InstrumentId::from_raw(1))
            .expect("micro_price Some");
        assert_eq!(mp, Ticks::from_raw(50));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn micro_price_weighted_toward_larger_qty_side() {
        // bid=40 qty=300, ask=60 qty=100  → total=400
        // micro = (40*100 + 60*300) / 400 = (4000 + 18000)/400 = 22000/400 = 55
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 300, 60, 100);
        let mp = engine
            .micro_price(InstrumentId::from_raw(1))
            .expect("micro_price Some");
        assert_eq!(mp, Ticks::from_raw(55));
    }

    #[test]
    fn micro_price_none_for_invalid_instrument() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(engine.micro_price(InstrumentId::NONE).is_none());
        assert!(engine.micro_price(InstrumentId::from_raw(5)).is_none());
    }

    #[test]
    fn micro_price_none_with_empty_book() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(engine.micro_price(InstrumentId::from_raw(1)).is_none());
    }

    // -----------------------------------------------------------------------
    // check_bbo_price_change() — catches: wrong price-change detection
    // -----------------------------------------------------------------------

    #[test]
    fn check_bbo_no_tob_on_size_only_change() {
        // After the first snapshot (which emits a TOB), update only quantity —
        // price unchanged → no new TOB should be emitted.
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 10, 60, 10);
        let _ = engine.take_tob(); // consume first TOB

        // Re-insert same prices but different quantities via another snapshot
        let snap1 = make_delta(1, 40, 999, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, 60, 888, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
        assert!(
            engine.take_tob().is_none(),
            "size-only change must not emit TOB"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn check_bbo_emits_tob_on_bid_price_change() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 10, 60, 10);
        let _ = engine.take_tob();

        // Move bid price from 40 → 41
        let snap1 = make_delta(1, 41, 10, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, 60, 10, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
        let tob = engine.take_tob().expect("bid price change should emit TOB");
        assert_eq!(tob.bid_price, Ticks::from_raw(41));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn check_bbo_emits_tob_on_ask_price_change() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 10, 60, 10);
        let _ = engine.take_tob();

        // Move ask price from 60 → 61
        let snap1 = make_delta(1, 40, 10, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap2 = make_delta(1, 61, 10, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap1);
        engine.process(&snap2);
        let tob = engine.take_tob().expect("ask price change should emit TOB");
        assert_eq!(tob.ask_price, Ticks::from_raw(61));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn check_bbo_tob_spread_field_is_correct() {
        // bid=40, ask=50  →  spread = 10 in the emitted TOB
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        setup_book(&mut engine, 40, 10, 50, 10);
        let tob = engine.take_tob().expect("TOB from snapshot");
        assert_eq!(
            tob.spread,
            Ticks::from_raw(10),
            "spread in TOB must be ask - bid"
        );
    }

    // -----------------------------------------------------------------------
    // process() Trade arm — catches: Trade arm deleted mutation
    // -----------------------------------------------------------------------

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn process_trade_event_updates_last_trade() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(engine.last_trade(InstrumentId::from_raw(1)).is_none());
        let ev = make_trade(1, 500, 25, Side::Bid, 9_000);
        engine.process(&ev);
        // If Trade arm were deleted, last_trade would still be None
        let trade = engine
            .last_trade(InstrumentId::from_raw(1))
            .expect("Trade arm must have stored a trade");
        assert_eq!(trade.price, Ticks::from_raw(500));
        assert_eq!(trade.qty, Lots::from_raw(25));
        assert_eq!(trade.side, Side::Bid);
    }

    #[test]
    fn process_trade_out_of_range_instrument_ignored() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        // instrument 5 is out of range for active_count=4
        let ev = make_trade(5, 500, 25, Side::Bid, 9_000);
        engine.process(&ev);
        // Should not panic; all instruments still have no trade
        for raw in 1..=4_u32 {
            assert!(engine.last_trade(InstrumentId::from_raw(raw)).is_none());
        }
    }

    // -----------------------------------------------------------------------
    // process() TopOfBook arm — catches: TopOfBook arm deleted mutation
    // -----------------------------------------------------------------------

    #[test]
    fn process_top_of_book_event_marks_ready() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        assert!(!engine.is_ready(InstrumentId::from_raw(1)));
        let ev = make_top_of_book(1, 40, 30, 50, 20, 2_000);
        engine.process(&ev);
        // If TopOfBook arm were deleted, is_ready would still be false
        assert!(engine.is_ready(InstrumentId::from_raw(1)));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn process_top_of_book_event_emits_tob() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let ev = make_top_of_book(1, 40, 30, 50, 20, 2_000);
        engine.process(&ev);
        // If TopOfBook arm were deleted, no TOB would be produced
        let tob = engine
            .take_tob()
            .expect("TopOfBook event must produce a TOB");
        assert_eq!(tob.bid_price, Ticks::from_raw(40));
        assert_eq!(tob.ask_price, Ticks::from_raw(50));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertion")]
    fn process_top_of_book_event_correct_spread_and_micro() {
        // bid=40 qty=100, ask=60 qty=100 → spread=20, micro=50
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let ev = make_top_of_book(1, 40, 100, 60, 100, 3_000);
        engine.process(&ev);
        let tob = engine.take_tob().expect("TOB");
        assert_eq!(tob.spread, Ticks::from_raw(20));
        assert_eq!(tob.micro_price, Ticks::from_raw(50));
    }

    #[test]
    fn process_top_of_book_out_of_range_instrument_ignored() {
        let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);
        let ev = make_top_of_book(5, 40, 30, 50, 20, 1_000);
        engine.process(&ev);
        assert!(engine.take_tob().is_none());
    }
}
