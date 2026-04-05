//! Instrument identifier.

use core::fmt;

/// Instrument identifier.
///
/// Identifies a tradeable instrument in the system.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct InstrumentId(u32);

impl InstrumentId {
    /// Sentinel value for system/control events with no associated instrument.
    ///
    /// INVARIANT: 0 is permanently reserved. It must never correspond to a real instrument.
    pub const NONE: Self = Self(0);

    /// Construct from a raw `u32` identifier.
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    /// Extract the raw `u32` identifier.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        self.0
    }
}

impl Default for InstrumentId {
    fn default() -> Self {
        Self::NONE
    }
}

impl fmt::Debug for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InstrumentId({})", self.0)
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn size() {
        assert_eq!(core::mem::size_of::<InstrumentId>(), 4);
    }

    #[test]
    fn construction() {
        let id = InstrumentId::from_raw(42);
        assert_eq!(id.to_raw(), 42);
    }

    #[test]
    fn none_sentinel() {
        assert_eq!(InstrumentId::NONE.to_raw(), 0);
    }

    #[test]
    fn ordering() {
        let a = InstrumentId::from_raw(1);
        let b = InstrumentId::from_raw(2);
        assert!(a < b);
        assert_eq!(a, a);
    }

    #[test]
    fn display_and_debug() {
        let id = InstrumentId::from_raw(99);
        assert_eq!(id.to_string(), "99");
        assert_eq!(alloc::format!("{id:?}"), "InstrumentId(99)");
    }
}
