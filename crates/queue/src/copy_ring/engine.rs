//! Copy-optimized ring engine for `T: Copy` types.
//!
//! Same SPSC protocol as `RingEngine` but uses `CopyPolicy` for slot
//! operations and returns `bool` instead of `Result` (caller retains
//! the value since `T: Copy`).

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::storage::Storage;
use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_platform::{CachePadded, CopyPolicy};

/// Cold slow-path for full ring. `#[cold]` tells LLVM to move this
/// out of the hot path even on stable.
#[cold]
#[inline(never)]
fn slow_full() -> bool {
    false
}

/// Cold slow-path for empty ring.
#[cold]
#[inline(never)]
fn slow_empty() -> bool {
    false
}

pub(crate) struct CopyRingEngine<T: Copy, S, I, P, Instr, CP> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    tail_cached: CachePadded<Cell<usize>>,
    head_cached: CachePadded<Cell<usize>>,
    storage: S,
    instr: Instr,
    _marker: PhantomData<(T, I, P, CP)>,
}

impl<T, S, I, P, Instr, CP> CopyRingEngine<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
    pub(crate) fn new(storage: S, instr: Instr) -> Self {
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            tail_cached: CachePadded::new(Cell::new(0)),
            head_cached: CachePadded::new(Cell::new(0)),
            storage,
            instr,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn push(&self, value: &T) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = I::wrap(head + 1, self.storage.capacity());

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_write(&self.storage, next_head);

        if next_head == self.tail_cached.get() {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            if next_head == tail {
                core::hint::cold_path();
                self.instr.on_push_full();
                return slow_full();
            }
        }

        crate::copy_ring::raw::write_slot_copy::<T, S, CP>(&self.storage, head, value);
        self.head.store(next_head, Ordering::Release);
        self.instr.on_push();
        true
    }

    #[inline]
    pub(crate) fn pop(&self, out: &mut T) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_read(&self.storage, tail);

        if tail == self.head_cached.get() {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            if tail == head {
                core::hint::cold_path();
                self.instr.on_pop_empty();
                return slow_empty();
            }
        }

        crate::copy_ring::raw::read_slot_copy::<T, S, CP>(&self.storage, tail, out);
        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.tail.store(next_tail, Ordering::Release);
        self.instr.on_pop();
        true
    }

    #[inline]
    pub(crate) fn push_batch(&self, src: &[T]) -> usize {
        if src.is_empty() {
            return 0;
        }

        let head = self.head.load(Ordering::Relaxed);
        let cached_tail = self.tail_cached.get();
        let cap = self.storage.capacity();
        let usable = cap - 1;

        let len = if head >= cached_tail {
            head - cached_tail
        } else {
            cap - cached_tail + head
        };
        let mut free = usable - len;

        if free < src.len() {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            let len = if head >= tail {
                head - tail
            } else {
                cap - tail + head
            };
            free = usable - len;
            if free == 0 {
                return 0;
            }
        }

        let n = src.len().min(free);

        // Two-chunk contiguous copy bypassing per-element CopyPolicy dispatch.
        // `memcpy` auto-vectorizes for bulk transfers; per-element SIMD
        // dispatch adds call overhead that dominates for large batches.
        //
        // `first_chunk`: slots from head to end of backing array (no wrap).
        // `second_chunk`: remaining slots written from index 0 (wrap).
        let first_chunk = n.min(cap - head);
        let second_chunk = n - first_chunk;

        // First chunk: slots head..head+first_chunk (no wrap).
        // `head < cap` (ring invariant) and `first_chunk <= cap - head`,
        // so `head + first_chunk <= cap`. Producer owns this range.
        crate::copy_ring::raw::write_batch_copy::<T, S>(&self.storage, head, &src[..first_chunk]);

        if second_chunk > 0 {
            // Second chunk: wraps to slots 0..second_chunk.
            // `second_chunk <= n - first_chunk <= free < cap`,
            // so `second_chunk <= cap`. Producer owns this range.
            crate::copy_ring::raw::write_batch_copy::<T, S>(&self.storage, 0, &src[first_chunk..n]);
        }

        self.head.store(I::wrap(head + n, cap), Ordering::Release);
        n
    }

    #[inline]
    pub(crate) fn pop_batch(&self, dst: &mut [T]) -> usize {
        if dst.is_empty() {
            return 0;
        }

        let tail = self.tail.load(Ordering::Relaxed);
        let cached_head = self.head_cached.get();
        let cap = self.storage.capacity();

        let mut avail = if cached_head >= tail {
            cached_head - tail
        } else {
            cap - tail + cached_head
        };

        if avail < dst.len() {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            avail = if head >= tail {
                head - tail
            } else {
                cap - tail + head
            };
            if avail == 0 {
                return 0;
            }
        }

        let n = dst.len().min(avail);

        // Two-chunk contiguous copy symmetric to push_batch.
        // `memcpy` auto-vectorizes for bulk transfers; per-element dispatch
        // adds call overhead that dominates for large batches.
        //
        // `first_chunk`: slots from tail to end of backing array (no wrap).
        // `second_chunk`: remaining slots read from index 0 (wrap).
        let first_chunk = n.min(cap - tail);
        let second_chunk = n - first_chunk;

        // First chunk: slots tail..tail+first_chunk (no wrap).
        // `tail < cap` (ring invariant) and `first_chunk <= cap - tail`,
        // so `tail + first_chunk <= cap`. Consumer owns this range.
        crate::copy_ring::raw::read_batch_copy::<T, S>(
            &self.storage,
            tail,
            &mut dst[..first_chunk],
        );

        if second_chunk > 0 {
            // Second chunk: wraps to slots 0..second_chunk.
            // `second_chunk <= n - first_chunk <= avail < cap`,
            // so `second_chunk <= cap`. Consumer owns this range.
            crate::copy_ring::raw::read_batch_copy::<T, S>(
                &self.storage,
                0,
                &mut dst[first_chunk..n],
            );
        }

        self.tail.store(I::wrap(tail + n, cap), Ordering::Release);
        n
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity() - 1
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let cap = self.storage.capacity();
        if head >= tail {
            head - tail
        } else {
            cap - tail + head
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn instrumentation(&self) -> &Instr {
        &self.instr
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::storage::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    type TestEngine = CopyRingEngine<
        u64,
        InlineStorage<u64, 8>,
        Pow2Masked,
        ImmediatePush,
        NoInstr,
        mantis_platform::DefaultCopyPolicy,
    >;

    fn new_engine() -> TestEngine {
        CopyRingEngine::new(InlineStorage::new(), NoInstr)
    }

    #[test]
    fn push_pop_single() {
        let engine = new_engine();
        let val = 42u64;
        let mut out = 0u64;
        assert!(engine.push(&val));
        assert!(engine.pop(&mut out));
        assert_eq!(out, 42);
    }

    #[test]
    fn push_full_returns_false() {
        let engine = new_engine();
        // capacity = 8, usable = 7 (sentinel slot)
        for i in 0u64..7 {
            assert!(engine.push(&i), "push {i} should succeed");
        }
        assert!(!engine.push(&99), "push to full ring should return false");
    }

    #[test]
    fn pop_empty_returns_false() {
        let engine = new_engine();
        let mut out = 0u64;
        assert!(
            !engine.pop(&mut out),
            "pop from empty ring should return false"
        );
    }

    #[test]
    fn fifo_ordering() {
        let engine = new_engine();
        for i in 0u64..7 {
            assert!(engine.push(&i));
        }
        for i in 0u64..7 {
            let mut out = 0u64;
            assert!(engine.pop(&mut out));
            assert_eq!(out, i);
        }
    }

    #[test]
    fn wraparound() {
        let engine = new_engine();
        // Push 5, pop 5, push 5 again — exercises index wrapping
        for round in 0..3 {
            for i in 0u64..5 {
                assert!(engine.push(&(round * 10 + i)));
            }
            for i in 0u64..5 {
                let mut out = 0u64;
                assert!(engine.pop(&mut out));
                assert_eq!(out, round * 10 + i);
            }
        }
    }

    #[test]
    fn push_batch_full_capacity() {
        let engine = new_engine();
        let src: Vec<u64> = (0..7).collect();
        let pushed = engine.push_batch(&src);
        assert_eq!(pushed, 7);

        let more = [100u64, 101];
        assert_eq!(engine.push_batch(&more), 0);
    }

    #[test]
    fn pop_batch_all() {
        let engine = new_engine();
        let src: Vec<u64> = (0..5).collect();
        engine.push_batch(&src);

        let mut dst = vec![0u64; 5];
        let popped = engine.pop_batch(&mut dst);
        assert_eq!(popped, 5);
        assert_eq!(dst, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn push_batch_partial() {
        let engine = new_engine();
        let first: Vec<u64> = (0..5).collect();
        assert_eq!(engine.push_batch(&first), 5);

        let second: Vec<u64> = (10..15).collect();
        assert_eq!(engine.push_batch(&second), 2);
    }

    #[test]
    fn pop_batch_partial() {
        let engine = new_engine();
        let src: Vec<u64> = (0..3).collect();
        engine.push_batch(&src);

        let mut dst = vec![0u64; 10];
        let popped = engine.pop_batch(&mut dst);
        assert_eq!(popped, 3);
        assert_eq!(&dst[..3], &[0, 1, 2]);
    }

    #[test]
    fn batch_empty_slice() {
        let engine = new_engine();
        assert_eq!(engine.push_batch(&[]), 0);
        let mut dst = vec![0u64; 0];
        assert_eq!(engine.pop_batch(&mut dst), 0);
    }

    #[test]
    fn batch_wraparound() {
        let engine = new_engine();
        let vals: Vec<u64> = (0..6).collect();
        engine.push_batch(&vals);
        let mut drain = vec![0u64; 6];
        engine.pop_batch(&mut drain);

        let wrap: Vec<u64> = (100..105).collect();
        assert_eq!(engine.push_batch(&wrap), 5);
        let mut out = vec![0u64; 5];
        assert_eq!(engine.pop_batch(&mut out), 5);
        assert_eq!(out, vec![100, 101, 102, 103, 104]);
    }

    #[test]
    fn pop_batch_wraparound_contiguous() {
        let engine = new_engine(); // capacity=8, usable=7
        // Advance tail to near end
        let fill: Vec<u64> = (0..6).collect();
        engine.push_batch(&fill);
        let mut drain = vec![0u64; 6];
        engine.pop_batch(&mut drain);

        // Push 5 (wraps around), then batch-pop all 5
        let wrap_src: Vec<u64> = (300..305).collect();
        engine.push_batch(&wrap_src);

        let mut out = vec![0u64; 5];
        let popped = engine.pop_batch(&mut out);
        assert_eq!(popped, 5);
        assert_eq!(out, vec![300, 301, 302, 303, 304]);
    }

    #[test]
    fn push_batch_wraparound_contiguous() {
        // Advance head to near end of buffer, then batch-push across wrap
        let engine = new_engine(); // capacity=8, usable=7
        // Fill 6, drain 6 — head and tail now at index 6
        let fill: Vec<u64> = (0..6).collect();
        engine.push_batch(&fill);
        let mut drain = vec![0u64; 6];
        engine.pop_batch(&mut drain);

        // Now push 5 elements starting at index 6: wraps at index 8 -> 0
        let wrap_src: Vec<u64> = (200..205).collect();
        let pushed = engine.push_batch(&wrap_src);
        assert_eq!(pushed, 5);

        let mut out = vec![0u64; 5];
        let popped = engine.pop_batch(&mut out);
        assert_eq!(popped, 5);
        assert_eq!(out, vec![200, 201, 202, 203, 204]);
    }

    #[test]
    fn batch_fifo_ordering() {
        let engine = new_engine();
        for round in 0u64..10 {
            let src: Vec<u64> = (round * 7..(round + 1) * 7).collect();
            let pushed = engine.push_batch(&src);
            assert_eq!(pushed, src.len());

            let mut dst = vec![0u64; pushed];
            let popped = engine.pop_batch(&mut dst);
            assert_eq!(popped, pushed);
            assert_eq!(dst, src);
        }
    }

    #[test]
    fn len_and_capacity() {
        let engine = new_engine();
        assert_eq!(engine.capacity(), 7); // 8 - 1 sentinel
        assert_eq!(engine.len(), 0);
        assert!(engine.is_empty());

        engine.push(&1);
        assert_eq!(engine.len(), 1);
        assert!(!engine.is_empty());

        let mut out = 0u64;
        engine.pop(&mut out);
        assert_eq!(engine.len(), 0);
    }
}
