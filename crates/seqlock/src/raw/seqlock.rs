//! Core `SeqLock` implementation.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicUsize;

use mantis_platform::{CachePadded, CopyPolicy, DefaultCopyPolicy};

/// Lock-free sequence lock. Single writer, multiple readers.
///
/// Writer: `store(&mut self, val)` — updates the protected value.
/// Reader: `load(&self) -> T` — reads the latest value, retrying on contention.
#[repr(C)]
pub struct SeqLock<T: Copy, C: CopyPolicy<T> = DefaultCopyPolicy> {
    seq: CachePadded<AtomicUsize>,
    data: UnsafeCell<MaybeUninit<T>>,
    _copy: PhantomData<C>,
}

// SAFETY: SeqLock is designed for cross-thread sharing.
// The writer uses &mut self (single-writer guaranteed by borrow checker).
// Readers use &self with atomic sequence checking for consistency.
// T: Send is required because the value crosses thread boundaries.
unsafe impl<T: Copy + Send, C: CopyPolicy<T>> Sync for SeqLock<T, C> {}
unsafe impl<T: Copy + Send, C: CopyPolicy<T>> Send for SeqLock<T, C> {}

use core::sync::atomic::Ordering;

impl<T: Copy, C: CopyPolicy<T>> SeqLock<T, C> {
    /// Create a new seqlock with an initial value.
    /// Sequence starts at 0 (even = consistent).
    #[inline]
    pub fn new(initial: T) -> Self {
        Self {
            seq: CachePadded::new(AtomicUsize::new(0)),
            data: UnsafeCell::new(MaybeUninit::new(initial)),
            _copy: PhantomData,
        }
    }

    /// Read the current sequence number.
    /// Even = consistent state. Odd = write in progress.
    /// Useful for "has it changed since last check?" patterns.
    #[inline]
    pub fn version(&self) -> usize {
        self.seq.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_initializes_with_value() {
        let lock = SeqLock::<u64>::new(42);
        assert_eq!(lock.version(), 0);
    }

    #[test]
    fn new_initializes_with_array() {
        let lock = SeqLock::<[u64; 4]>::new([1, 2, 3, 4]);
        assert_eq!(lock.version(), 0);
    }
}
