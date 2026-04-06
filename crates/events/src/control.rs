//! Control event payloads and supporting enums.

use core::fmt;

/// The category of a timer event.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TimerKind {
    /// Fired when a market data feed has not ticked within the expected window.
    StaleFeed = 0,
    /// Fired at a regular, configurable cadence.
    Periodic = 1,
    /// Fired when a wall-clock deadline has been reached.
    Deadline = 2,
}

impl fmt::Debug for TimerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleFeed => write!(f, "TimerKind::StaleFeed"),
            Self::Periodic => write!(f, "TimerKind::Periodic"),
            Self::Deadline => write!(f, "TimerKind::Deadline"),
        }
    }
}

/// A timer expiry event.
///
/// Timers are first-class events, not callbacks.
///
/// Strategies receive timer expirations through the normal event loop rather
/// than via separate callback registration, keeping the execution model
/// single-threaded and branch-free.
#[derive(Clone, Copy)]
#[repr(C)]
#[expect(clippy::pub_underscore_fields, reason = "repr(C) padding field")]
pub struct TimerPayload {
    /// Application-defined identifier used to distinguish multiple timers.
    pub timer_id: u32,
    /// The category of timer that fired.
    pub kind: TimerKind,
    /// Reserved padding — must be zero.
    pub _pad: [u8; 3],
}

/// An internal liveness ping.
///
/// Internal liveness event, not a feed-level heartbeat.
///
/// Used to confirm that the event loop is advancing and that upstream
/// producers are still alive. It is not forwarded to downstream consumers.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct HeartbeatPayload {
    /// Monotonically increasing counter incremented by the producer.
    pub counter: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn timer_kind_repr_values() {
        assert_eq!(TimerKind::StaleFeed as u8, 0);
        assert_eq!(TimerKind::Periodic as u8, 1);
        assert_eq!(TimerKind::Deadline as u8, 2);
    }

    #[test]
    fn timer_kind_size_is_1() {
        assert_eq!(mem::size_of::<TimerKind>(), 1);
    }

    #[test]
    fn timer_payload_size_is_8() {
        assert_eq!(mem::size_of::<TimerPayload>(), 8);
    }

    #[test]
    fn heartbeat_payload_size_is_4() {
        assert_eq!(mem::size_of::<HeartbeatPayload>(), 4);
    }

    #[test]
    fn timer_roundtrip() {
        let p = TimerPayload {
            timer_id: 3,
            kind: TimerKind::Periodic,
            _pad: [0; 3],
        };
        assert_eq!(p.timer_id, 3);
        assert_eq!(p.kind, TimerKind::Periodic);
    }

    #[test]
    fn heartbeat_roundtrip() {
        let p = HeartbeatPayload { counter: 42 };
        assert_eq!(p.counter, 42);
    }

    // --- mutant-catching: field distinctness and variant coverage ---

    #[test]
    fn timer_kind_stale_feed_repr() {
        assert_eq!(TimerKind::StaleFeed as u8, 0);
    }

    #[test]
    fn timer_kind_periodic_repr() {
        assert_eq!(TimerKind::Periodic as u8, 1);
    }

    #[test]
    fn timer_kind_deadline_repr() {
        assert_eq!(TimerKind::Deadline as u8, 2);
    }

    #[test]
    fn timer_kind_variants_are_distinct() {
        assert_ne!(TimerKind::StaleFeed as u8, TimerKind::Periodic as u8);
        assert_ne!(TimerKind::Periodic as u8, TimerKind::Deadline as u8);
        assert_ne!(TimerKind::StaleFeed as u8, TimerKind::Deadline as u8);
    }

    #[test]
    fn timer_payload_stale_feed_roundtrip() {
        let p = TimerPayload {
            timer_id: 0,
            kind: TimerKind::StaleFeed,
            _pad: [0; 3],
        };
        assert_eq!(p.timer_id, 0);
        assert_eq!(p.kind, TimerKind::StaleFeed);
    }

    #[test]
    fn timer_payload_deadline_roundtrip() {
        let p = TimerPayload {
            timer_id: u32::MAX,
            kind: TimerKind::Deadline,
            _pad: [0; 3],
        };
        assert_eq!(p.timer_id, u32::MAX);
        assert_eq!(p.kind, TimerKind::Deadline);
    }

    #[test]
    fn heartbeat_counter_zero() {
        let p = HeartbeatPayload { counter: 0 };
        assert_eq!(p.counter, 0);
    }

    #[test]
    fn heartbeat_counter_max() {
        let p = HeartbeatPayload { counter: u32::MAX };
        assert_eq!(p.counter, u32::MAX);
    }

    #[test]
    fn timer_id_is_independent_of_kind() {
        let p1 = TimerPayload { timer_id: 1, kind: TimerKind::Periodic, _pad: [0; 3] };
        let p2 = TimerPayload { timer_id: 2, kind: TimerKind::Periodic, _pad: [0; 3] };
        assert_ne!(p1.timer_id, p2.timer_id);
        assert_eq!(p1.kind, p2.kind);
    }
}
