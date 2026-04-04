//! Core `SeqLock` implementation.
//!
//! Three optimizations over the baseline seqlock:
//!
//! 1. **SIMD copy via `CopyPolicy`** — reader uses 128-bit NEON/SSE2 loads
//!    instead of byte-at-a-time `read_volatile`. Shorter copy window = fewer retries.
//!
//! 2. **Platform-specific fence** — `compiler_fence` on `x86_64` (TSO gives us
//!    hardware ordering for free), `fence` on ARM64 (weak memory needs it).
//!    Matches rigtorp's approach on x86, stays correct on ARM.
//!
//! 3. **Prefetch** — data cache line is prefetched before reading the sequence
//!    counter, so the copy hits L1 instead of L2/L3.

use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::AtomicUsize;

use mantis_platform::{
    CachePadded, CopyPolicy, DefaultCopyPolicy, PrefetchLocality, PrefetchRW, prefetch,
};

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

/// Platform-specific read barrier for the seqlock protocol.
///
/// On `x86_64` (TSO): compiler fence only — hardware provides store-load ordering.
/// On ARM64 (weak memory): full acquire fence — required for correctness.
///
/// This matches rigtorp's approach on x86 while staying correct on ARM.
#[expect(
    clippy::inline_always,
    reason = "hot-path fence must inline — function call overhead defeats the purpose"
)]
#[inline(always)]
fn seqlock_read_barrier() {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: On x86_64, TSO guarantees that loads are not reordered with
        // other loads. A compiler fence is sufficient to prevent the compiler
        // from reordering the data copy past the seq2 load.
        core::sync::atomic::compiler_fence(Ordering::Acquire);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // On weak-memory architectures (ARM64), a hardware fence is required
        // to ensure the data copy completes before we read seq2.
        core::sync::atomic::fence(Ordering::Acquire);
    }
}

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
    /// # Optimizations
    ///
    /// - **Write-prefetch**: brings data cache line into Modified state before
    ///   the write, avoiding the RFO (Read For Ownership) stall.
    /// - **SIMD copy**: uses `CopyPolicy::copy_in` for wide stores on larger types.
    ///
    /// # Ordering
    ///
    /// Two `Release` stores on the sequence counter bracket the data write:
    /// - First store (odd) makes "write in progress" visible to readers
    /// - Second store (even) makes the completed data visible to readers
    #[inline]
    pub fn store(&mut self, val: T) {
        // Write-prefetch: bring data cache line into Modified (exclusive) state.
        // Eliminates the RFO stall on the actual write.
        prefetch(
            self.data.get().cast_const().cast::<u8>(),
            PrefetchRW::Write,
            PrefetchLocality::High,
        );
        let seq = self.seq.load(Ordering::Relaxed);
        // Odd sequence signals "write in progress" to readers.
        // SAFETY: Release ordering ensures this store is visible before
        // the data write that follows.
        self.seq.store(seq.wrapping_add(1), Ordering::Release);
        // Single-writer guaranteed by &mut self. No concurrent writes.
        // UnsafeCell::get() provides raw pointer access for interior mutation.
        // CopyPolicy::copy_in enables SIMD-accelerated wide stores.
        C::copy_in(self.data.get().cast::<T>(), core::ptr::addr_of!(val));
        // Even sequence signals "write complete, data consistent".
        // SAFETY: Release ordering ensures the data write above is visible
        // before this sequence update.
        self.seq.store(seq.wrapping_add(2), Ordering::Release);
    }

    /// Load the current value. Lock-free. Retries on contention.
    ///
    /// Multiple readers can call this concurrently on `&self`.
    ///
    /// # Optimizations
    ///
    /// - **Prefetch**: data cache line brought into L1 before the copy
    /// - **SIMD copy**: `CopyPolicy` enables 128-bit NEON/SSE2 loads
    /// - **Platform fence**: `compiler_fence` on x86 (TSO), `fence` on ARM
    ///
    /// # Ordering
    ///
    /// 1. Prefetch data cache line
    /// 2. `Acquire` load of seq1 — ensures we see latest sequence
    /// 3. Copy data via `CopyPolicy` — may be torn if writer is active
    /// 4. Platform-specific read barrier
    /// 5. `Relaxed` load of seq2 — barrier provides the ordering
    /// 6. If seq1 == seq2 and even, data is consistent — return it
    #[inline]
    pub fn load(&self) -> T {
        loop {
            // Prefetch data into L1 — by the time we pass the seq1 check,
            // the cache line is ready for the copy.
            prefetch(
                self.data.get().cast_const().cast::<u8>(),
                PrefetchRW::Read,
                PrefetchLocality::High,
            );
            let seq1 = self.seq.load(Ordering::Acquire);
            if seq1 & 1 != 0 {
                // Writer active — don't waste time copying, spin immediately
                core::hint::spin_loop();
                continue;
            }
            // SAFETY: We copy through UnsafeCell into a local MaybeUninit.
            // The data may be torn if a writer is concurrent — that's OK,
            // we check the sequence after and discard torn reads.
            // CopyPolicy::copy_out is used instead of read_volatile to enable
            // SIMD-accelerated wide loads (128-bit NEON/SSE2).
            let val = unsafe {
                let mut dst = MaybeUninit::<T>::uninit();
                C::copy_out(dst.as_mut_ptr(), self.data.get().cast::<T>());
                dst.assume_init()
            };
            // Platform-specific barrier: compiler_fence on x86 (TSO),
            // hardware fence on ARM (weak memory).
            seqlock_read_barrier();
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
