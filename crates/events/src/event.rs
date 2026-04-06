//! Hot event transport unit combining header and body.

use core::fmt;

use crate::body::{EventBody, EventKind};
use crate::control::{HeartbeatPayload, TimerPayload};
use crate::execution::{FillPayload, OrderAckPayload, OrderRejectPayload};
use crate::flags::EventFlags;
use crate::header::EventHeader;
use crate::market::{BookDeltaPayload, TopOfBookPayload, TradePayload};
use mantis_types::{InstrumentId, SeqNum, SourceId, Timestamp};

/// Hot event transport unit.
///
/// Target: ≤64 bytes, `Copy`, `repr(C)`.
/// The `header` field is always at offset 0 so callers can read the header
/// without matching on the body variant.
///
/// # Layout
///
/// ```text
/// offset  0: header (24 bytes) — EventHeader
/// offset 24: body   (40 bytes) — EventBody (2-byte discriminant + 6-byte pad + 32-byte variant)
/// ```
///
/// Total: 64 bytes, align 8.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HotEvent {
    /// Fixed-size header present on every event.
    pub header: EventHeader,
    /// Discriminated payload carrying the event-specific data.
    pub body: EventBody,
}

// Hard gate — must not regress past one cache line.
const _: () = assert!(core::mem::size_of::<HotEvent>() <= 64);

impl HotEvent {
    /// Returns the [`EventKind`] discriminant for this event.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        self.body.kind()
    }

    /// Constructs a [`BookDelta`](EventBody::BookDelta) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn book_delta(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: BookDeltaPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::BookDelta(payload),
        }
    }

    /// Constructs a [`Trade`](EventBody::Trade) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn trade(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: TradePayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::Trade(payload),
        }
    }

    /// Constructs a [`TopOfBook`](EventBody::TopOfBook) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn top_of_book(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: TopOfBookPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::TopOfBook(payload),
        }
    }

    /// Constructs an [`OrderAck`](EventBody::OrderAck) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn order_ack(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: OrderAckPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::OrderAck(payload),
        }
    }

    /// Constructs a [`Fill`](EventBody::Fill) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn fill(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: FillPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::Fill(payload),
        }
    }

    /// Constructs an [`OrderReject`](EventBody::OrderReject) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn order_reject(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: OrderRejectPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::OrderReject(payload),
        }
    }

    /// Constructs a [`Timer`](EventBody::Timer) event.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "header fields are semantically distinct; a builder would add indirection"
    )]
    pub const fn timer(
        recv_ts: Timestamp,
        seq: SeqNum,
        instrument_id: InstrumentId,
        source_id: SourceId,
        flags: EventFlags,
        payload: TimerPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id,
                source_id,
                flags,
            },
            body: EventBody::Timer(payload),
        }
    }

    /// Constructs a [`Heartbeat`](EventBody::Heartbeat) event.
    ///
    /// Heartbeats are not instrument-specific; [`InstrumentId::NONE`] is used
    /// automatically so callers do not need to supply a meaningless instrument
    /// identifier.
    #[must_use]
    pub const fn heartbeat(
        recv_ts: Timestamp,
        seq: SeqNum,
        source_id: SourceId,
        flags: EventFlags,
        payload: HeartbeatPayload,
    ) -> Self {
        Self {
            header: EventHeader {
                recv_ts,
                seq,
                instrument_id: InstrumentId::NONE,
                source_id,
                flags,
            },
            body: EventBody::Heartbeat(payload),
        }
    }
}

impl fmt::Debug for HotEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HotEvent")
            .field("kind", &self.kind())
            .field("header", &self.header)
            .finish_non_exhaustive()
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
    use crate::flags::EventFlags;
    use crate::market::{BookDeltaPayload, TopOfBookPayload, TradePayload, UpdateAction};

    #[test]
    fn hot_event_size_ceiling() {
        assert!(mem::size_of::<HotEvent>() <= 64);
    }

    #[test]
    fn header_at_offset_zero() {
        assert_eq!(mem::offset_of!(HotEvent, header), 0);
    }

    #[test]
    fn hot_event_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<HotEvent>();
    }

    #[test]
    fn constructor_book_delta() {
        let ts = Timestamp::from_nanos(1_000);
        let seq = SeqNum::from_raw(42);
        let iid = InstrumentId::from_raw(7);
        let sid = SourceId::from_raw(3);
        let flags = EventFlags::EMPTY;
        let payload = BookDeltaPayload {
            price: Ticks::from_raw(100),
            qty: Lots::from_raw(10),
            side: Side::Bid,
            action: UpdateAction::New,
            depth: 0,
            _pad: [0; 5],
        };
        let event = HotEvent::book_delta(ts, seq, iid, sid, flags, payload);
        assert_eq!(event.kind(), EventKind::BookDelta);
        assert_eq!(event.header.recv_ts, ts);
        assert_eq!(event.header.seq, seq);
        assert_eq!(event.header.instrument_id, iid);
        assert_eq!(event.header.source_id, sid);
    }

    #[test]
    fn constructor_heartbeat_uses_none_instrument() {
        let ts = Timestamp::from_nanos(0);
        let seq = SeqNum::ZERO;
        let sid = SourceId::from_raw(1);
        let flags = EventFlags::EMPTY;
        let payload = HeartbeatPayload { counter: 1 };
        let event = HotEvent::heartbeat(ts, seq, sid, flags, payload);
        assert_eq!(event.kind(), EventKind::Heartbeat);
        assert_eq!(event.header.instrument_id, InstrumentId::NONE);
    }

    #[test]
    fn constructor_timer() {
        let ts = Timestamp::from_nanos(500);
        let seq = SeqNum::from_raw(1);
        let iid = InstrumentId::from_raw(0);
        let sid = SourceId::from_raw(2);
        let flags = EventFlags::EMPTY;
        let payload = TimerPayload {
            timer_id: 99,
            kind: TimerKind::Deadline,
            _pad: [0; 3],
        };
        let event = HotEvent::timer(ts, seq, iid, sid, flags, payload);
        assert_eq!(event.kind(), EventKind::Timer);
        if let EventBody::Timer(p) = event.body {
            assert_eq!(p.timer_id, 99);
            assert_eq!(p.kind, TimerKind::Deadline);
        } else {
            assert_eq!(event.kind(), EventKind::Timer, "body must be Timer variant");
        }
    }

    #[test]
    fn event_copy_semantics() {
        let ts = Timestamp::from_nanos(42);
        let seq = SeqNum::from_raw(1);
        let iid = InstrumentId::from_raw(5);
        let sid = SourceId::from_raw(1);
        let flags = EventFlags::IS_SNAPSHOT;
        let payload = TradePayload {
            price: Ticks::from_raw(300),
            qty: Lots::from_raw(2),
            aggressor: Side::Ask,
            _pad: [0; 7],
        };
        let original = HotEvent::trade(ts, seq, iid, sid, flags, payload);
        let copy = original;
        assert_eq!(copy.kind(), EventKind::Trade);
        assert_eq!(copy.header.recv_ts, original.header.recv_ts);
        assert_eq!(copy.header.seq, original.header.seq);
    }

    #[test]
    fn kind_convenience_matches_body_kind() {
        let payload = OrderAckPayload {
            order_id: OrderId::from_raw(1),
            client_order_id: 2,
            status: OrderStatus::Accepted,
            _pad: [0; 7],
        };
        let event = HotEvent::order_ack(
            Timestamp::from_nanos(0),
            SeqNum::ZERO,
            InstrumentId::from_raw(1),
            SourceId::from_raw(1),
            EventFlags::EMPTY,
            payload,
        );
        assert_eq!(event.kind(), event.body.kind());
    }

    #[test]
    fn constructor_fill() {
        let payload = FillPayload {
            order_id: OrderId::from_raw(7),
            price: Ticks::from_raw(500),
            qty: Lots::from_raw(3),
            side: Side::Bid,
            is_maker: 1,
            _pad: [0; 6],
        };
        let event = HotEvent::fill(
            Timestamp::from_nanos(0),
            SeqNum::ZERO,
            InstrumentId::from_raw(1),
            SourceId::from_raw(1),
            EventFlags::EMPTY,
            payload,
        );
        assert_eq!(event.kind(), EventKind::Fill);
    }

    #[test]
    fn constructor_order_reject() {
        let payload = OrderRejectPayload {
            order_id: OrderId::from_raw(0),
            client_order_id: 55,
            reason: RejectReason::InvalidPrice,
            _pad: [0; 7],
        };
        let event = HotEvent::order_reject(
            Timestamp::from_nanos(0),
            SeqNum::ZERO,
            InstrumentId::from_raw(1),
            SourceId::from_raw(1),
            EventFlags::EMPTY,
            payload,
        );
        assert_eq!(event.kind(), EventKind::OrderReject);
    }

    #[test]
    fn constructor_top_of_book() {
        let payload = TopOfBookPayload {
            bid_price: Ticks::from_raw(99),
            bid_qty: Lots::from_raw(50),
            ask_price: Ticks::from_raw(101),
            ask_qty: Lots::from_raw(30),
        };
        let event = HotEvent::top_of_book(
            Timestamp::from_nanos(0),
            SeqNum::ZERO,
            InstrumentId::from_raw(1),
            SourceId::from_raw(1),
            EventFlags::LAST_IN_BATCH,
            payload,
        );
        assert_eq!(event.kind(), EventKind::TopOfBook);
        assert!(event.header.flags.contains(EventFlags::LAST_IN_BATCH));
    }

    // --- mutant-catching: header field propagation ---

    #[test]
    fn constructor_trade_header_fields() {
        let ts = Timestamp::from_nanos(12_345);
        let seq = SeqNum::from_raw(99);
        let iid = InstrumentId::from_raw(42);
        let sid = SourceId::from_raw(7);
        let flags = EventFlags::IS_SNAPSHOT;
        let payload = TradePayload {
            price: Ticks::from_raw(300),
            qty: Lots::from_raw(2),
            aggressor: Side::Bid,
            _pad: [0; 7],
        };
        let event = HotEvent::trade(ts, seq, iid, sid, flags, payload);
        assert_eq!(event.kind(), EventKind::Trade);
        assert_eq!(event.header.recv_ts, ts);
        assert_eq!(event.header.seq, seq);
        assert_eq!(event.header.instrument_id, iid);
        assert_eq!(event.header.source_id, sid);
        assert!(event.header.flags.contains(EventFlags::IS_SNAPSHOT));
        assert_eq!(event.kind(), EventKind::Trade);
        if let EventBody::Trade(p) = event.body {
            assert_eq!(p.aggressor, Side::Bid);
            assert_eq!(p.price, Ticks::from_raw(300));
        }
    }

    #[test]
    fn constructor_book_delta_snapshot_flag() {
        let flags = EventFlags::IS_SNAPSHOT | EventFlags::LAST_IN_BATCH;
        let payload = BookDeltaPayload {
            price: Ticks::from_raw(50),
            qty: Lots::from_raw(100),
            side: Side::Ask,
            action: UpdateAction::New,
            depth: 2,
            _pad: [0; 5],
        };
        let event = HotEvent::book_delta(
            Timestamp::from_nanos(0),
            SeqNum::ZERO,
            InstrumentId::from_raw(1),
            SourceId::from_raw(1),
            flags,
            payload,
        );
        assert!(event.header.flags.contains(EventFlags::IS_SNAPSHOT));
        assert!(event.header.flags.contains(EventFlags::LAST_IN_BATCH));
        assert_eq!(event.kind(), EventKind::BookDelta);
        if let EventBody::BookDelta(p) = event.body {
            assert_eq!(p.side, Side::Ask);
            assert_eq!(p.depth, 2);
        }
    }

    #[test]
    fn constructor_heartbeat_counter_propagates() {
        let payload = HeartbeatPayload { counter: 77 };
        let event = HotEvent::heartbeat(
            Timestamp::from_nanos(0),
            SeqNum::ZERO,
            SourceId::from_raw(1),
            EventFlags::EMPTY,
            payload,
        );
        assert_eq!(event.kind(), EventKind::Heartbeat);
        assert_eq!(event.kind(), EventKind::Heartbeat);
        if let EventBody::Heartbeat(p) = event.body {
            assert_eq!(p.counter, 77);
        }
    }

    #[test]
    fn seq_num_propagates_correctly() {
        // Ensure seq field is actually stored, not silently dropped
        let seq_a = SeqNum::from_raw(1);
        let seq_b = SeqNum::from_raw(2);
        let make_event = |seq: SeqNum| {
            HotEvent::heartbeat(
                Timestamp::from_nanos(0),
                seq,
                SourceId::from_raw(1),
                EventFlags::EMPTY,
                HeartbeatPayload { counter: 0 },
            )
        };
        assert_eq!(make_event(seq_a).header.seq, seq_a);
        assert_eq!(make_event(seq_b).header.seq, seq_b);
        assert_ne!(make_event(seq_a).header.seq, seq_b);
    }
}
