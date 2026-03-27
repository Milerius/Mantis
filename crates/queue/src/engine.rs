//! Core SPSC ring buffer engine.
//!
//! `RingEngine` is the internal engine that implements the
//! Acquire/Release ring buffer protocol with cached remote indices.

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::storage::Storage;
use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_platform::CachePadded;
use mantis_types::{PushError, QueueError};

/// Producer-local cache line: own head position + remote tail cache.
pub(crate) struct ProducerCache {
    pub(crate) head_local: Cell<usize>,
    pub(crate) tail_remote: Cell<usize>,
}

/// Consumer-local cache line: own tail position + remote head cache.
pub(crate) struct ConsumerCache {
    pub(crate) tail_local: Cell<usize>,
    pub(crate) head_remote: Cell<usize>,
}

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
    producer: CachePadded<ProducerCache>,
    consumer: CachePadded<ConsumerCache>,
    storage: S,
    instr: Instr,
    _marker: PhantomData<(T, I, P)>,
}

// NOTE: `unsafe impl Sync` lives in `raw/mod.rs` per unsafe isolation policy.

#[cold]
#[inline(never)]
fn slow_push_full<T>(value: T) -> Result<(), PushError<T>> {
    Err(PushError::Full(value))
}

#[cold]
#[inline(never)]
fn slow_pop_empty<T>() -> Result<T, QueueError> {
    Err(QueueError::Empty)
}

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
            producer: CachePadded::new(ProducerCache {
                head_local: Cell::new(0),
                tail_remote: Cell::new(0),
            }),
            consumer: CachePadded::new(ConsumerCache {
                tail_local: Cell::new(0),
                head_remote: Cell::new(0),
            }),
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
        // On ARM, Cell read avoids implicit ordering overhead of atomic
        // self-load. On x86 (TSO), Relaxed atomic is already a plain mov.
        #[cfg(target_arch = "aarch64")]
        let head = self.producer.head_local.get();
        #[cfg(not(target_arch = "aarch64"))]
        let head = self.head.load(Ordering::Relaxed);

        let next_head = I::wrap(head + 1, self.storage.capacity());

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_write(&self.storage, next_head);

        if next_head == self.producer.tail_remote.get() {
            let tail = self.tail.load(Ordering::Acquire);
            self.producer.tail_remote.set(tail);
            if next_head == tail {
                core::hint::cold_path();
                self.instr.on_push_full();
                return slow_push_full(value);
            }
        }

        crate::raw::write_slot(&self.storage, head, value);
        self.head.store(next_head, Ordering::Release);
        #[cfg(target_arch = "aarch64")]
        self.producer.head_local.set(next_head);
        self.instr.on_push();
        Ok(())
    }

    #[inline]
    pub(crate) fn try_pop(&self) -> Result<T, QueueError> {
        #[cfg(target_arch = "aarch64")]
        let tail = self.consumer.tail_local.get();
        #[cfg(not(target_arch = "aarch64"))]
        let tail = self.tail.load(Ordering::Relaxed);

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_read(&self.storage, tail);

        if tail == self.consumer.head_remote.get() {
            let head = self.head.load(Ordering::Acquire);
            self.consumer.head_remote.set(head);
            if tail == head {
                core::hint::cold_path();
                self.instr.on_pop_empty();
                return slow_pop_empty();
            }
        }

        let value = crate::raw::read_slot(&self.storage, tail);
        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.tail.store(next_tail, Ordering::Release);
        #[cfg(target_arch = "aarch64")]
        self.consumer.tail_local.set(next_tail);
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
