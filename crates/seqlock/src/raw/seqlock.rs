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

use core::sync::atomic::{Ordering, fence};

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

    /// Store a new value. Single-writer only — enforced by `&mut self`.
    ///
    /// Lock-free, wait-free. Never blocks. O(1).
    ///
    /// # Ordering
    ///
    /// Two `Release` stores on the sequence counter bracket the data write:
    /// - First store (odd) makes "write in progress" visible to readers
    /// - Second store (even) makes the completed data visible to readers
    #[inline]
    pub fn store(&mut self, val: T) {
        let seq = self.seq.load(Ordering::Relaxed);
        // Odd sequence signals "write in progress" to readers.
        // SAFETY: Release ordering ensures this store is visible before
        // the data write that follows.
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        // SAFETY: Single-writer guaranteed by &mut self. No concurrent writes.
        // UnsafeCell allows interior mutation. MaybeUninit accepts any bit pattern.
        unsafe {
            core::ptr::write(self.data.get().cast::<T>(), val);
        }
        // Even sequence signals "write complete, data consistent".
        // SAFETY: Release ordering ensures the data write above is visible
        // before this sequence update.
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// Load the current value. Lock-free. Retries on contention.
    ///
    /// Multiple readers can call this concurrently on `&self`.
    ///
    /// # Ordering
    ///
    /// 1. `Acquire` load of seq1 — ensures we see latest sequence
    /// 2. Volatile read of data — may be torn if writer is active
    /// 3. `Acquire` fence — prevents seq2 load from reordering before data copy
    /// 4. `Relaxed` load of seq2 — fence provides the ordering
    /// 5. If seq1 == seq2 and even, data is consistent — return it
    #[inline]
    pub fn load(&self) -> T {
        loop {
            let seq1 = self.seq.load(Ordering::Acquire);
            if seq1 & 1 != 0 {
                // Writer active — don't waste time copying, spin immediately
                core::hint::spin_loop();
                continue;
            }
            // SAFETY: We read through UnsafeCell via volatile read.
            // The data may be torn if a writer is concurrent — that's OK,
            // we check the sequence after and discard torn reads.
            // read_volatile prevents the compiler from eliding this read.
            let val = unsafe { core::ptr::read_volatile(self.data.get() as *const T) };
            // Prevent the CPU/compiler from reordering the seq2 load
            // before the data copy completes.
            fence(Ordering::Acquire);
            let seq2 = self.seq.load(Ordering::Relaxed);
            if seq1 == seq2 {
                return val;
            }
            // Writer snuck in — discard torn read, retry
            core::hint::spin_loop();
        }
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

    #[test]
    fn store_increments_version_by_two() {
        let mut lock = SeqLock::<u64>::new(0);
        assert_eq!(lock.version(), 0);
        lock.store(42);
        assert_eq!(lock.version(), 2);
        lock.store(99);
        assert_eq!(lock.version(), 4);
    }

    #[test]
    fn store_version_always_even_after_complete() {
        let mut lock = SeqLock::<u64>::new(0);
        for i in 0..1000u64 {
            lock.store(i);
            let v = lock.version();
            assert_eq!(v & 1, 0, "version {v} is odd after store");
        }
    }

    #[test]
    fn load_returns_initial_value() {
        let lock = SeqLock::<u64>::new(42);
        assert_eq!(lock.load(), 42);
    }

    #[test]
    fn load_returns_latest_stored_value() {
        let mut lock = SeqLock::<u64>::new(0);
        lock.store(100);
        assert_eq!(lock.load(), 100);
        lock.store(200);
        assert_eq!(lock.load(), 200);
    }

    #[test]
    fn load_works_with_array_payload() {
        let mut lock = SeqLock::<[u64; 4]>::new([0; 4]);
        lock.store([10, 20, 30, 40]);
        assert_eq!(lock.load(), [10, 20, 30, 40]);
    }

    #[test]
    fn load_works_with_large_payload() {
        let mut lock = SeqLock::<[u8; 128]>::new([0u8; 128]);
        let val = [0xAB_u8; 128];
        lock.store(val);
        assert_eq!(lock.load(), val);
    }

    #[test]
    fn store_load_roundtrip_many_sizes() {
        // 8 bytes
        let mut lock8 = SeqLock::<u64>::new(0);
        lock8.store(0xDEAD_BEEF_CAFE_1234);
        assert_eq!(lock8.load(), 0xDEAD_BEEF_CAFE_1234);

        // 32 bytes
        let mut lock32 = SeqLock::<[u64; 4]>::new([0; 4]);
        lock32.store([1, 2, 3, 4]);
        assert_eq!(lock32.load(), [1, 2, 3, 4]);

        // 64 bytes
        let mut lock64 = SeqLock::<[u8; 64]>::new([0; 64]);
        lock64.store([0xFF; 64]);
        assert_eq!(lock64.load(), [0xFF; 64]);
    }
}
