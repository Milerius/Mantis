//! Core types, newtypes, and error definitions for the Mantis SDK.
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

/// Error type for queue operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The queue is full and cannot accept more items.
    Full,
    /// The queue is empty and has no items to return.
    Empty,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "queue is full"),
            Self::Empty => write!(f, "queue is empty"),
        }
    }
}

/// Sequence number for tracking event ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeqNum(pub u64);

/// Index into a ring buffer slot array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotIndex(pub usize);

/// Compile-time assertion that a capacity is a power of two.
///
/// # Panics
///
/// Panics at compile time if `N` is not a power of two or is zero.
pub struct AssertPowerOfTwo<const N: usize>;

impl<const N: usize> AssertPowerOfTwo<N> {
    /// Const assertion. Call in a `const { }` block to validate at compile time.
    pub const VALID: () = assert!(
        N.is_power_of_two() && N > 0,
        "capacity must be a non-zero power of two"
    );
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn queue_error_display() {
        assert_eq!(QueueError::Full.to_string(), "queue is full");
        assert_eq!(QueueError::Empty.to_string(), "queue is empty");
    }

    #[test]
    fn seq_num_ordering() {
        assert!(SeqNum(1) < SeqNum(2));
        assert_eq!(SeqNum(42), SeqNum(42));
    }

    #[test]
    fn slot_index_equality() {
        assert_eq!(SlotIndex(0), SlotIndex(0));
        assert_ne!(SlotIndex(0), SlotIndex(1));
    }

    #[test]
    fn power_of_two_valid() {
        let () = AssertPowerOfTwo::<1>::VALID;
        let () = AssertPowerOfTwo::<2>::VALID;
        let () = AssertPowerOfTwo::<1024>::VALID;
    }
}
