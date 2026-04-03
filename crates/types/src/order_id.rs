//! Order identifier.

use core::fmt;

/// Order identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct OrderId(u64);

impl OrderId {
    /// Construct from a raw `u64` identifier.
    #[must_use]
    pub const fn from_raw(id: u64) -> Self {
        Self(id)
    }

    /// Extract the raw `u64` identifier.
    #[must_use]
    pub const fn to_raw(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrderId({})", self.0)
    }
}

impl fmt::Display for OrderId {
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
    fn construction() {
        let id = OrderId::from_raw(42);
        assert_eq!(id.to_raw(), 42);
    }

    #[test]
    fn ordering() {
        let a = OrderId::from_raw(1);
        let b = OrderId::from_raw(2);
        assert!(a < b);
        assert_eq!(a, a);
    }

    #[test]
    fn display_and_debug() {
        let id = OrderId::from_raw(99);
        assert_eq!(id.to_string(), "99");
        assert_eq!(alloc::format!("{id:?}"), "OrderId(99)");
    }
}
