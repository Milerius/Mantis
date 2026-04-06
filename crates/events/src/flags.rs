//! Cross-cutting event metadata flags carried in every [`EventHeader`].
//!
//! [`EventHeader`]: crate::EventHeader

use core::fmt;
use core::ops::{BitOr, BitOrAssign};

/// Bitflags encoding cross-cutting metadata for a hot event.
///
/// These flags travel in every [`EventHeader`] and let consumers quickly
/// test event properties without inspecting the payload union.
///
/// [`EventHeader`]: crate::EventHeader
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct EventFlags(u16);

impl EventFlags {
    /// No flags set — the default state.
    pub const EMPTY: Self = Self(0);

    /// The event is part of a snapshot (full state replay), not a live delta.
    pub const IS_SNAPSHOT: Self = Self(1 << 0);

    /// This is the last event in the current batch.
    ///
    /// Consumers may use this to flush downstream aggregations.
    pub const LAST_IN_BATCH: Self = Self(1 << 1);

    /// Construct flags from a raw `u16` value.
    #[must_use]
    #[inline]
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    /// Return the underlying `u16` representation.
    #[must_use]
    #[inline]
    pub const fn to_raw(self) -> u16 {
        self.0
    }

    /// Return `true` if all bits in `other` are set in `self`.
    #[must_use]
    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Return a new `EventFlags` with all bits from `other` added.
    #[must_use]
    #[inline]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Return `true` if no flags are set.
    #[must_use]
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for EventFlags {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for EventFlags {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl fmt::Debug for EventFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EventFlags(0x{:04x})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_by_default() {
        assert!(EventFlags::EMPTY.is_empty());
        assert_eq!(EventFlags::EMPTY.to_raw(), 0);
    }

    #[test]
    fn set_single_flag() {
        let f = EventFlags::IS_SNAPSHOT;
        assert!(f.contains(EventFlags::IS_SNAPSHOT));
        assert!(!f.contains(EventFlags::LAST_IN_BATCH));
        assert!(!f.is_empty());
    }

    #[test]
    fn combine_flags() {
        let f = EventFlags::IS_SNAPSHOT.with(EventFlags::LAST_IN_BATCH);
        assert!(f.contains(EventFlags::IS_SNAPSHOT));
        assert!(f.contains(EventFlags::LAST_IN_BATCH));
    }

    #[test]
    fn bitor_operator() {
        let f = EventFlags::IS_SNAPSHOT | EventFlags::LAST_IN_BATCH;
        assert!(f.contains(EventFlags::IS_SNAPSHOT));
        assert!(f.contains(EventFlags::LAST_IN_BATCH));
    }

    #[test]
    fn bitor_assign() {
        let mut f = EventFlags::IS_SNAPSHOT;
        f |= EventFlags::LAST_IN_BATCH;
        assert!(f.contains(EventFlags::IS_SNAPSHOT));
        assert!(f.contains(EventFlags::LAST_IN_BATCH));
    }

    #[test]
    fn from_raw_roundtrip() {
        let raw: u16 = 0x0003;
        assert_eq!(EventFlags::from_raw(raw).to_raw(), raw);
    }

    #[test]
    fn size_is_2() {
        assert_eq!(core::mem::size_of::<EventFlags>(), 2);
    }

    // --- mutant-catching: raw bit value checks ---

    #[test]
    fn is_snapshot_raw_bit_is_1() {
        assert_eq!(EventFlags::IS_SNAPSHOT.to_raw(), 1 << 0);
    }

    #[test]
    fn last_in_batch_raw_bit_is_2() {
        assert_eq!(EventFlags::LAST_IN_BATCH.to_raw(), 1 << 1);
    }

    #[test]
    fn is_snapshot_does_not_contain_last_in_batch() {
        assert!(!EventFlags::IS_SNAPSHOT.contains(EventFlags::LAST_IN_BATCH));
    }

    #[test]
    fn last_in_batch_does_not_contain_is_snapshot() {
        assert!(!EventFlags::LAST_IN_BATCH.contains(EventFlags::IS_SNAPSHOT));
    }

    #[test]
    fn empty_does_not_contain_is_snapshot() {
        assert!(!EventFlags::EMPTY.contains(EventFlags::IS_SNAPSHOT));
    }

    #[test]
    fn empty_does_not_contain_last_in_batch() {
        assert!(!EventFlags::EMPTY.contains(EventFlags::LAST_IN_BATCH));
    }

    #[test]
    fn snapshot_flag_is_not_empty() {
        assert!(!EventFlags::IS_SNAPSHOT.is_empty());
    }

    #[test]
    fn last_in_batch_flag_is_not_empty() {
        assert!(!EventFlags::LAST_IN_BATCH.is_empty());
    }

    #[test]
    fn with_combines_without_mutation() {
        let base = EventFlags::IS_SNAPSHOT;
        let combined = base.with(EventFlags::LAST_IN_BATCH);
        // base must be unchanged
        assert!(!base.contains(EventFlags::LAST_IN_BATCH));
        // combined must have both
        assert!(combined.contains(EventFlags::IS_SNAPSHOT));
        assert!(combined.contains(EventFlags::LAST_IN_BATCH));
        assert_eq!(combined.to_raw(), 0x0003);
    }

    #[test]
    fn contains_self_is_true() {
        assert!(EventFlags::IS_SNAPSHOT.contains(EventFlags::IS_SNAPSHOT));
        assert!(EventFlags::LAST_IN_BATCH.contains(EventFlags::LAST_IN_BATCH));
        assert!(EventFlags::EMPTY.contains(EventFlags::EMPTY));
    }

    #[test]
    fn bitor_assign_does_not_clear_existing() {
        let mut f = EventFlags::IS_SNAPSHOT | EventFlags::LAST_IN_BATCH;
        f |= EventFlags::EMPTY;
        assert!(f.contains(EventFlags::IS_SNAPSHOT));
        assert!(f.contains(EventFlags::LAST_IN_BATCH));
    }
}
