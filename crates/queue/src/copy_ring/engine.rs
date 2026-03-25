//! Copy-optimized ring engine for `T: Copy` types.
//!
//! Same SPSC protocol as `RingEngine` but uses `CopyPolicy` for slot
//! operations and returns `bool` instead of `Result` (caller retains
//! the value since `T: Copy`).

// Public handles (Producer/Consumer split wrappers) are added in a later task.
// Until then, all items in this module are reachable only from tests.
#![allow(dead_code)]

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use mantis_core::{CopyPolicy, IndexStrategy, Instrumentation, PushPolicy};

use crate::pad::CachePadded;
use crate::storage::Storage;

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

        if next_head == self.tail_cached.get() {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            if next_head == tail {
                #[cfg(feature = "nightly")]
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

        if tail == self.head_cached.get() {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            if tail == head {
                #[cfg(feature = "nightly")]
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
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity() - 1
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let cap = self.storage.capacity();
        if head >= tail { head - tail } else { cap - tail + head }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn instrumentation(&self) -> &Instr {
        &self.instr
    }

    #[inline]
    pub(crate) fn storage(&self) -> &S {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    type TestEngine = CopyRingEngine<
        u64,
        InlineStorage<u64, 8>,
        Pow2Masked,
        ImmediatePush,
        NoInstr,
        crate::copy_ring::raw::simd::DefaultCopyPolicy,
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
        assert!(!engine.pop(&mut out), "pop from empty ring should return false");
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
