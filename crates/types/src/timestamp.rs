//! Nanosecond-precision epoch timestamp.

use core::fmt;

/// Nanosecond-precision epoch timestamp.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Zero timestamp (epoch).
    pub const ZERO: Self = Self(0);

    /// Construct from a nanosecond count.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Extract the nanosecond count.
    #[must_use]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({}ns)", self.0)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn construction() {
        let ts = Timestamp::from_nanos(1_000_000_000);
        assert_eq!(ts.as_nanos(), 1_000_000_000);
    }

    #[test]
    fn zero() {
        assert_eq!(Timestamp::ZERO.as_nanos(), 0);
    }

    #[test]
    fn ordering() {
        let a = Timestamp::from_nanos(100);
        let b = Timestamp::from_nanos(200);
        assert!(a < b);
        assert_eq!(a, a);
    }

    #[test]
    fn display_and_debug() {
        let ts = Timestamp::from_nanos(42);
        assert_eq!(ts.to_string(), "42ns");
        assert_eq!(alloc::format!("{ts:?}"), "Timestamp(42ns)");
    }
}
