//! Market side.

use core::fmt;

/// Market side: bid or ask.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Side {
    /// Buying / bid side.
    #[default]
    Bid = 0,
    /// Selling / ask side.
    Ask = 1,
}

impl Side {
    /// Returns the opposite side.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Bid => Self::Ask,
            Self::Ask => Self::Bid,
        }
    }
}

impl fmt::Debug for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bid => write!(f, "Bid"),
            Self::Ask => write!(f, "Ask"),
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bid => write!(f, "Bid"),
            Self::Ask => write!(f, "Ask"),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn opposite_bid_is_ask() {
        assert_eq!(Side::Bid.opposite(), Side::Ask);
    }

    #[test]
    fn opposite_ask_is_bid() {
        assert_eq!(Side::Ask.opposite(), Side::Bid);
    }

    #[test]
    fn double_opposite_identity() {
        assert_eq!(Side::Bid.opposite().opposite(), Side::Bid);
        assert_eq!(Side::Ask.opposite().opposite(), Side::Ask);
    }

    #[test]
    fn repr_values() {
        assert_eq!(Side::Bid as u8, 0);
        assert_eq!(Side::Ask as u8, 1);
    }

    #[test]
    fn display_and_debug() {
        assert_eq!(Side::Bid.to_string(), "Bid");
        assert_eq!(Side::Ask.to_string(), "Ask");
        assert_eq!(alloc::format!("{:?}", Side::Bid), "Bid");
        assert_eq!(alloc::format!("{:?}", Side::Ask), "Ask");
    }
}
