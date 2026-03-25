//! Core traits and strategy definitions for the Mantis SDK.
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

/// Defines how head/tail indices wrap around the buffer capacity.
pub trait IndexStrategy {
    /// Wrap a raw index to a valid slot position.
    fn wrap(index: usize, capacity: usize) -> usize;
}

/// Defines push behavior when the queue is full.
pub trait PushPolicy {
    /// Returns `true` if the push should block/spin when full.
    fn should_block() -> bool;
}

/// Defines measurement hooks for instrumentation.
pub trait Instrumentation {
    /// Called after a successful push.
    fn on_push(&self) {}
    /// Called after a successful pop.
    fn on_pop(&self) {}
    /// Called when a push fails due to full queue.
    fn on_push_full(&self) {}
    /// Called when a pop fails due to empty queue.
    fn on_pop_empty(&self) {}
}

/// Power-of-2 masked index strategy. Wraps via bitwise AND.
pub struct Pow2Masked;

impl IndexStrategy for Pow2Masked {
    #[inline]
    fn wrap(index: usize, capacity: usize) -> usize {
        debug_assert!(capacity.is_power_of_two(), "capacity must be power of 2");
        index & (capacity - 1)
    }
}

/// Push immediately returns `Err(Full)` when queue is full.
pub struct ImmediatePush;

impl PushPolicy for ImmediatePush {
    #[inline]
    fn should_block() -> bool {
        false
    }
}

/// No-op instrumentation. Zero overhead in release builds.
pub struct NoInstr;

impl Instrumentation for NoInstr {}
