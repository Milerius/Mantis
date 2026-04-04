//! The common header carried by every hot event.

use crate::EventFlags;
use mantis_types::{InstrumentId, SeqNum, SourceId, Timestamp};

/// Fixed-size header prepended to every hot event.
///
/// Layout (`repr(C)`, 24 bytes, align 8):
///
/// ```text
/// offset  0: recv_ts       (8 bytes) — nanosecond receive timestamp
/// offset  8: seq           (8 bytes) — per-source sequence number
/// offset 16: instrument_id (4 bytes) — instrument identifier
/// offset 20: source_id     (2 bytes) — feed/venue source identifier
/// offset 22: flags         (2 bytes) — cross-cutting event flags
/// ```
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct EventHeader {
    /// Nanosecond wall-clock timestamp at which the event was received.
    pub recv_ts: Timestamp,
    /// Monotonically increasing sequence number assigned by the source.
    pub seq: SeqNum,
    /// Identifies the financial instrument this event relates to.
    pub instrument_id: InstrumentId,
    /// Identifies the upstream feed or venue that produced this event.
    pub source_id: SourceId,
    /// Cross-cutting metadata flags (snapshot, last-in-batch, …).
    pub flags: EventFlags,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn size_is_24() {
        assert_eq!(mem::size_of::<EventHeader>(), 24);
    }

    #[test]
    fn alignment_is_8() {
        assert_eq!(mem::align_of::<EventHeader>(), 8);
    }

    #[test]
    fn construction() {
        let h = EventHeader {
            recv_ts: Timestamp::from_nanos(1_000_000),
            seq: SeqNum::from_raw(1),
            instrument_id: InstrumentId::from_raw(42),
            source_id: SourceId::from_raw(7),
            flags: EventFlags::EMPTY,
        };
        assert_eq!(h.recv_ts, Timestamp::from_nanos(1_000_000));
        assert_eq!(h.seq, SeqNum::from_raw(1));
        assert_eq!(h.instrument_id, InstrumentId::from_raw(42));
        assert_eq!(h.source_id, SourceId::from_raw(7));
        assert!(h.flags.is_empty());
    }

    #[test]
    fn flags_roundtrip() {
        let h = EventHeader {
            recv_ts: Timestamp::from_nanos(0),
            seq: SeqNum::from_raw(0),
            instrument_id: InstrumentId::from_raw(0),
            source_id: SourceId::from_raw(0),
            flags: EventFlags::IS_SNAPSHOT | EventFlags::LAST_IN_BATCH,
        };
        assert!(h.flags.contains(EventFlags::IS_SNAPSHOT));
        assert!(h.flags.contains(EventFlags::LAST_IN_BATCH));
    }

    #[test]
    fn recv_ts_at_offset_zero() {
        assert_eq!(core::mem::offset_of!(EventHeader, recv_ts), 0);
    }
}
