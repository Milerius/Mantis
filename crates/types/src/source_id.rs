//! Source identifier.

use core::fmt;

/// Identifies the feed or source that produced an event.
///
/// Named source constants are defined by the application, not by this crate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SourceId(u16);

impl SourceId {
    /// Construct from a raw `u16` identifier.
    #[must_use]
    pub const fn from_raw(id: u16) -> Self {
        Self(id)
    }

    /// Extract the raw `u16` identifier.
    #[must_use]
    pub const fn to_raw(self) -> u16 {
        self.0
    }
}

impl fmt::Debug for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SourceId({})", self.0)
    }
}

impl fmt::Display for SourceId {
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
        assert_eq!(core::mem::size_of::<SourceId>(), 2);
    }

    #[test]
    fn construction() {
        let id = SourceId::from_raw(7);
        assert_eq!(id.to_raw(), 7);
    }

    #[test]
    fn equality() {
        let a = SourceId::from_raw(1);
        let b = SourceId::from_raw(1);
        let c = SourceId::from_raw(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn display_and_debug() {
        let id = SourceId::from_raw(5);
        assert_eq!(id.to_string(), "5");
        assert_eq!(alloc::format!("{id:?}"), "SourceId(5)");
    }
}
