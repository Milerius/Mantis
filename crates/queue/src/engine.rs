//! Core SPSC ring buffer engine.
//!
//! `RingEngine` is the internal engine that implements the
//! Acquire/Release ring buffer protocol with cached remote indices.

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_types::{PushError, QueueError};

use crate::pad::CachePadded;
use crate::storage::Storage;

/// Generic SPSC ring engine. Not public -- use `RawRing` or split handles.
///
/// Contains `Cell<usize>` for cached indices, making it `!Sync` by default.
/// `unsafe impl Sync` is in `raw/mod.rs` -- justified by the SPSC protocol's
/// disjoint access guarantee.
pub(crate) struct RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    tail_cached: CachePadded<Cell<usize>>,
    head_cached: CachePadded<Cell<usize>>,
    storage: S,
    instr: Instr,
    _marker: PhantomData<(T, I, P)>,
}

// NOTE: `unsafe impl Sync` lives in `raw/mod.rs` per unsafe isolation policy.

impl<T, S, I, P, Instr> RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
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
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity() - 1
    }

    #[inline]
    pub(crate) fn try_push(&self, value: T) -> Result<(), PushError<T>> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = I::wrap(head + 1, self.storage.capacity());

        if next_head == self.tail_cached.get() {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            if next_head == tail {
                self.instr.on_push_full();
                return Err(PushError::Full(value));
            }
        }

        crate::raw::write_slot(&self.storage, head, value);
        self.head.store(next_head, Ordering::Release);
        self.instr.on_push();
        Ok(())
    }

    #[inline]
    pub(crate) fn try_pop(&self) -> Result<T, QueueError> {
        let tail = self.tail.load(Ordering::Relaxed);

        if tail == self.head_cached.get() {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            if tail == head {
                self.instr.on_pop_empty();
                return Err(QueueError::Empty);
            }
        }

        let value = crate::raw::read_slot(&self.storage, tail);
        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.tail.store(next_tail, Ordering::Release);
        self.instr.on_pop();
        Ok(value)
    }

    pub(crate) fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let cap = self.storage.capacity();
        (head + cap - tail) % cap
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn instrumentation(&self) -> &Instr {
        &self.instr
    }
}

impl<T, S, I, P, Instr> Drop for RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    fn drop(&mut self) {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        crate::raw::drop_range::<T, S, I>(&self.storage, tail, head);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    type TestEngine = RingEngine<u64, InlineStorage<u64, 4>, Pow2Masked, ImmediatePush, NoInstr>;

    #[test]
    fn push_pop_single() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        assert!(engine.try_push(42).is_ok());
        assert_eq!(engine.try_pop().ok(), Some(42));
    }

    #[test]
    fn push_full_returns_value() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        // capacity-1 = 3 usable slots (sentinel slot)
        assert!(engine.try_push(1).is_ok());
        assert!(engine.try_push(2).is_ok());
        assert!(engine.try_push(3).is_ok());
        let err = engine.try_push(4);
        assert_eq!(err, Err(PushError::Full(4)));
    }

    #[test]
    fn pop_empty_returns_error() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        assert_eq!(engine.try_pop(), Err(QueueError::Empty));
    }

    #[test]
    fn fifo_ordering() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        for i in 0..3 {
            assert!(engine.try_push(i).is_ok());
        }
        for i in 0..3 {
            assert_eq!(engine.try_pop().ok(), Some(i));
        }
    }

    #[test]
    fn wraparound() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        for round in 0..3 {
            for i in 0..3 {
                assert!(engine.try_push(round * 3 + i).is_ok());
            }
            for i in 0..3 {
                assert_eq!(engine.try_pop().ok(), Some(round * 3 + i));
            }
        }
    }

    #[test]
    fn len_and_is_empty() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        assert!(engine.is_empty());
        assert_eq!(engine.len(), 0);
        assert!(engine.try_push(1).is_ok());
        assert!(!engine.is_empty());
        assert_eq!(engine.len(), 1);
    }
}
