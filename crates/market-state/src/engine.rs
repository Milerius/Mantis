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
    pub fn micro_price(&self, inst: InstrumentId) -> Option<Ticks> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        let state = &self.instruments[slot - 1];
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
    pub fn book_imbalance(&self, inst: InstrumentId, levels: usize) -> Option<f64> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        let state = &self.instruments[slot - 1];
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
    pub fn spread(&self, inst: InstrumentId) -> Option<Ticks> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        let state = &self.instruments[slot - 1];
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
        now.as_nanos() - last > self.stale_timeout_ns
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

    /// Direct read-only access to the underlying order book.
    ///
    /// Returns `None` if the instrument ID is invalid.
    pub fn book(&self, inst: InstrumentId) -> Option<&B> {
        let slot = inst.to_raw() as usize;
        if slot == 0 || slot > self.active_count {
            return None;
        }
        Some(&self.instruments[slot - 1].book)
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
        let state = &self.instruments[slot];
        let bid = state.book.best_bid();
        let ask = state.book.best_ask();
        let bid_price = bid.map(|(p, _)| p);
        let ask_price = ask.map(|(p, _)| p);

        if bid_price == state.prev_bid && ask_price == state.prev_ask {
            return None;
        }

        let state = &mut self.instruments[slot];
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
    use mantis_events::{BookDeltaPayload, EventFlags, HotEvent};
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
        let snap =
            make_delta(1, 45, 100, Side::Bid, EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH));
        engine.process(&snap);
        // Within timeout: ts=1000, now=1050, delta=50 <= 100
        assert!(!engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1050)));
        // Beyond timeout: ts=1000, now=1200, delta=200 > 100
        assert!(engine.is_stale(InstrumentId::from_raw(1), Timestamp::from_nanos(1200)));
    }
}
