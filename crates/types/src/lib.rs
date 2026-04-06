//! Core types, newtypes, and error definitions for the Mantis SDK.
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

mod btc_qty;
mod instrument;
mod instrument_id;
mod lots;
mod order_id;
mod probability;
mod side;
mod signed_lots;
mod source_id;
mod ticks;
mod timestamp;
mod usdc;

pub use btc_qty::BtcQty;
pub use instrument::{InstrumentMeta, InstrumentMetaError};
pub use instrument_id::InstrumentId;
pub use lots::Lots;
pub use mantis_fixed::FixedI64;
pub use order_id::OrderId;
pub use probability::Probability;
pub use side::Side;
pub use signed_lots::SignedLots;
pub use source_id::SourceId;
pub use ticks::Ticks;
pub use timestamp::Timestamp;
pub use usdc::UsdcAmount;

use core::fmt;

/// Error returned when pushing to a full queue, preserving the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError<T> {
    /// The queue is full. Contains the value that was not pushed.
    Full(T),
}

impl<T> fmt::Display for PushError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(_) => write!(f, "queue is full"),
        }
    }
}

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

/// Per-queue monotonic sequence number. Not globally unique across queues.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SeqNum(u64);

impl SeqNum {
    /// Zero sequence number (start of a sequence).
    pub const ZERO: Self = Self(0);

    /// Construct from a raw `u64` value.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Extract the raw `u64` value.
    #[must_use]
    pub const fn to_raw(self) -> u64 {
        self.0
    }
}

impl fmt::Debug for SeqNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SeqNum({})", self.0)
    }
}

impl fmt::Display for SeqNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

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
    fn seq_num_construction() {
        let s = SeqNum::from_raw(7);
        assert_eq!(s.to_raw(), 7);
        assert_eq!(SeqNum::ZERO.to_raw(), 0);
    }

    #[test]
    fn seq_num_ordering() {
        assert!(SeqNum::from_raw(1) < SeqNum::from_raw(2));
        assert_eq!(SeqNum::from_raw(42), SeqNum::from_raw(42));
    }

    #[test]
    fn seq_num_display_and_debug() {
        let s = SeqNum::from_raw(99);
        assert_eq!(s.to_string(), "99");
        assert_eq!(alloc::format!("{s:?}"), "SeqNum(99)");
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

    #[test]
    fn push_error_preserves_value() {
        let err = PushError::Full(42u64);
        match err {
            PushError::Full(v) => assert_eq!(v, 42),
        }
    }

    #[test]
    fn push_error_display() {
        let err = PushError::Full(0u32);
        assert_eq!(err.to_string(), "queue is full");
    }
}
