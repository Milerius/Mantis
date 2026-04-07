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

/// Branch-based index wrapping. Uses a branch (predicted not-taken)
/// instead of bitwise AND. Faster on x86_64 where the branch predictor
/// learns the wrap-around almost never happens.
pub struct BranchWrap;

impl IndexStrategy for BranchWrap {
    #[inline(always)]
    fn wrap(index: usize, capacity: usize) -> usize {
        if index >= capacity { 0 } else { index }
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

use core::sync::atomic::{AtomicU64, Ordering};

/// Instrumentation that counts push/pop operations via atomic counters.
///
/// All increments use `Relaxed` ordering (counters are advisory, not
/// synchronization primitives). Suitable for debug/profiling presets.
pub struct CountingInstr {
    pushes: AtomicU64,
    pops: AtomicU64,
    push_full: AtomicU64,
    pop_empty: AtomicU64,
}

impl CountingInstr {
    /// Create a new counter with all values at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pushes: AtomicU64::new(0),
            pops: AtomicU64::new(0),
            push_full: AtomicU64::new(0),
            pop_empty: AtomicU64::new(0),
        }
    }

    /// Total successful pushes.
    #[must_use]
    pub fn push_count(&self) -> u64 {
        self.pushes.load(Ordering::Relaxed)
    }

    /// Total successful pops.
    #[must_use]
    pub fn pop_count(&self) -> u64 {
        self.pops.load(Ordering::Relaxed)
    }

    /// Total push attempts that failed (queue full).
    #[must_use]
    pub fn push_full_count(&self) -> u64 {
        self.push_full.load(Ordering::Relaxed)
    }

    /// Total pop attempts that failed (queue empty).
    #[must_use]
    pub fn pop_empty_count(&self) -> u64 {
        self.pop_empty.load(Ordering::Relaxed)
    }
}

impl Default for CountingInstr {
    fn default() -> Self {
        Self::new()
    }
}

impl Instrumentation for CountingInstr {
    #[inline]
    fn on_push(&self) {
        self.pushes.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_pop(&self) {
        self.pops.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_push_full(&self) {
        self.push_full.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_pop_empty(&self) {
        self.pop_empty.fetch_add(1, Ordering::Relaxed);
    }
}

/// No-op instrumentation. Zero overhead in release builds.
pub struct NoInstr;

impl Instrumentation for NoInstr {}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    #[test]
    fn counting_instr_tracks_push_pop() {
        let instr = CountingInstr::new();
        instr.on_push();
        instr.on_push();
        instr.on_pop();
        instr.on_push_full();
        instr.on_pop_empty();
        instr.on_pop_empty();
        assert_eq!(instr.push_count(), 2);
        assert_eq!(instr.pop_count(), 1);
        assert_eq!(instr.push_full_count(), 1);
        assert_eq!(instr.pop_empty_count(), 2);
    }

    #[test]
    fn branch_wrap_normal() {
        assert_eq!(BranchWrap::wrap(0, 1024), 0);
        assert_eq!(BranchWrap::wrap(500, 1024), 500);
        assert_eq!(BranchWrap::wrap(1023, 1024), 1023);
    }

    #[test]
    fn branch_wrap_at_boundary() {
        assert_eq!(BranchWrap::wrap(1024, 1024), 0);
        assert_eq!(BranchWrap::wrap(2048, 1024), 0);
    }
}
