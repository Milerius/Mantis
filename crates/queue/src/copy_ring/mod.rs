//! Copy-optimized SPSC ring buffer for `T: Copy` types.
//!
//! Provides SIMD-accelerated slot copies, batch push/pop, and
//! cold-path hints for sub-nanosecond hot paths.

pub(crate) mod engine;
#[cfg(feature = "alloc")]
pub(crate) mod handle;
pub(crate) mod raw;

use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_platform::CopyPolicy;

use crate::storage::Storage;
use engine::CopyRingEngine;

/// Public handle for the copy-optimized SPSC ring.
///
/// Analogous to `RawRing` but requires `T: Copy` and provides
/// batch operations and SIMD-accelerated copies.
pub struct RawRingCopy<T: Copy, S, I, P, Instr, CP> {
    engine: CopyRingEngine<T, S, I, P, Instr, CP>,
}

impl<T, S, I, P, Instr, CP> RawRingCopy<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
    pub(crate) fn with_strategies(storage: S, instr: Instr) -> Self {
        Self {
            engine: CopyRingEngine::new(storage, instr),
        }
    }

    /// Push a value by copying it into the ring.
    /// Returns `true` on success, `false` if the ring is full.
    #[inline]
    pub fn push(&mut self, value: &T) -> bool {
        self.engine.push(value)
    }

    /// Pop a value by copying it out of the ring.
    /// Returns `true` on success, `false` if the ring is empty.
    #[inline]
    pub fn pop(&mut self, out: &mut T) -> bool {
        self.engine.pop(out)
    }

    /// Push a batch of values. Returns the number actually pushed.
    #[inline]
    pub fn push_batch(&mut self, src: &[T]) -> usize {
        self.engine.push_batch(src)
    }

    /// Pop a batch of values. Returns the number actually popped.
    #[inline]
    pub fn pop_batch(&mut self, dst: &mut [T]) -> usize {
        self.engine.pop_batch(dst)
    }

    /// Usable capacity (storage capacity minus sentinel slot).
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.engine.capacity()
    }

    /// Current number of items in the ring.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.engine.len()
    }

    /// Whether the ring is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.engine.is_empty()
    }

    /// Access the instrumentation counters.
    #[inline]
    #[must_use]
    pub fn instrumentation(&self) -> &Instr {
        self.engine.instrumentation()
    }
}
