//! Curated preset type aliases for common SPSC ring configurations.

use mantis_core::{CountingInstr, ImmediatePush, NoInstr, Pow2Masked};

use crate::handle::RawRing;
use crate::storage::InlineStorage;

#[cfg(feature = "alloc")]
use crate::storage::HeapStorage;

/// Default SPSC ring — inline storage, no instrumentation.
pub type SpscRing<T, const N: usize> =
    RawRing<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr>;

impl<T: Send, const N: usize> SpscRing<T, N> {
    /// Create a new SPSC ring.
    #[must_use]
    pub fn new() -> Self {
        RawRing::with_strategies(InlineStorage::new(), NoInstr)
    }
}

impl<T: Send, const N: usize> Default for SpscRing<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Heap-allocated SPSC ring — runtime-sized.
#[cfg(feature = "alloc")]
pub type SpscRingHeap<T> = RawRing<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr>;

#[cfg(feature = "alloc")]
impl<T: Send> SpscRingHeap<T> {
    /// Create a new heap ring with at least `capacity` slots.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        RawRing::with_strategies(HeapStorage::new(capacity), NoInstr)
    }
}

/// Instrumented SPSC ring — tracks push/pop/full/empty counts.
pub type SpscRingInstrumented<T, const N: usize> =
    RawRing<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, CountingInstr>;

impl<T: Send, const N: usize> SpscRingInstrumented<T, N> {
    /// Create a new instrumented ring.
    #[must_use]
    pub fn new() -> Self {
        RawRing::with_strategies(InlineStorage::new(), CountingInstr::new())
    }
}

impl<T: Send, const N: usize> Default for SpscRingInstrumented<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spsc_ring_preset_works() {
        let mut ring = SpscRing::<u64, 8>::new();
        assert!(ring.try_push(1).is_ok());
        assert_eq!(ring.try_pop().ok(), Some(1));
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn heap_preset_works() {
        let mut ring = SpscRingHeap::<u64>::with_capacity(8);
        assert!(ring.try_push(3).is_ok());
        assert_eq!(ring.try_pop().ok(), Some(3));
    }

    #[test]
    fn instrumented_preset_tracks() {
        let mut ring = SpscRingInstrumented::<u64, 8>::new();
        assert!(ring.try_push(1).is_ok());
        let _ = ring.try_pop();
        let _ = ring.try_pop(); // will be empty
        let instr = ring.instrumentation();
        assert_eq!(instr.push_count(), 1);
        assert_eq!(instr.pop_count(), 1);
        assert_eq!(instr.pop_empty_count(), 1);
    }
}
