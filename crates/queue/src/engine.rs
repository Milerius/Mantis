//! Core SPSC ring buffer engine.
//!
//! `RingEngine` is the internal engine that implements the
//! Acquire/Release ring buffer protocol with cached remote indices.
//!
//! See `copy_ring::engine` module docs for the rationale behind
//! architecture-conditional local position caching (`head_local`/
//! `tail_local` on aarch64 only).

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::storage::Storage;
use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_platform::CacheLine;
use mantis_types::{PushError, QueueError};

/// Producer-local cache line: head atomic + cached remote tail.
/// Both accessed ONLY by the producer thread — colocated to minimize
/// cache footprint (1 cache line instead of 2).
#[repr(C)]
pub(crate) struct ProducerLine {
    pub(crate) head: AtomicUsize,
    #[cfg(target_arch = "aarch64")]
    pub(crate) head_local: Cell<usize>,
    pub(crate) tail_cached: Cell<usize>,
}

/// Consumer-local cache line: tail atomic + cached remote head.
/// Both accessed ONLY by the consumer thread.
#[repr(C)]
pub(crate) struct ConsumerLine {
    pub(crate) tail: AtomicUsize,
    #[cfg(target_arch = "aarch64")]
    pub(crate) tail_local: Cell<usize>,
    pub(crate) head_cached: Cell<usize>,
}

/// Generic SPSC ring engine. Not public -- use `RawRing` or split handles.
///
/// Contains `Cell<usize>` for cached indices, making it `!Sync` by default.
/// `unsafe impl Sync` is in `raw/mod.rs` -- justified by the SPSC protocol's
/// disjoint access guarantee.
///
/// Layout: 2 cache lines (producer + consumer) instead of 4 `CachePadded` fields.
pub(crate) struct RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    producer: CacheLine<ProducerLine>,
    consumer: CacheLine<ConsumerLine>,
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
            producer: CacheLine::new(ProducerLine {
                head: AtomicUsize::new(0),
                #[cfg(target_arch = "aarch64")]
                head_local: Cell::new(0),
                tail_cached: Cell::new(0),
            }),
            consumer: CacheLine::new(ConsumerLine {
                tail: AtomicUsize::new(0),
                #[cfg(target_arch = "aarch64")]
                tail_local: Cell::new(0),
                head_cached: Cell::new(0),
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

    #[inline(always)]
    pub(crate) fn try_push(&self, value: T) -> Result<(), PushError<T>> {
        // On ARM, Cell read avoids implicit ordering overhead of atomic
        // self-load. On x86 (TSO), Relaxed atomic is already a plain mov.
        #[cfg(target_arch = "aarch64")]
        let head = self.producer.head_local.get();
        #[cfg(not(target_arch = "aarch64"))]
        let head = self.producer.head.load(Ordering::Relaxed);

        let next_head = I::wrap(head + 1, self.storage.capacity());

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_write(&self.storage, next_head);

        if next_head == self.producer.tail_cached.get() {
            let tail = self.consumer.tail.load(Ordering::Acquire);
            self.producer.tail_cached.set(tail);
            if next_head == tail {
                core::hint::cold_path();
                self.instr.on_push_full();
                return slow_push_full(value);
            }
        }

        crate::raw::write_slot(&self.storage, head, value);
        self.producer.head.store(next_head, Ordering::Release);
        #[cfg(target_arch = "aarch64")]
        self.producer.head_local.set(next_head);
        self.instr.on_push();
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn try_pop(&self) -> Result<T, QueueError> {
        #[cfg(target_arch = "aarch64")]
        let tail = self.consumer.tail_local.get();
        #[cfg(not(target_arch = "aarch64"))]
        let tail = self.consumer.tail.load(Ordering::Relaxed);

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_read(&self.storage, tail);

        if tail == self.consumer.head_cached.get() {
            let head = self.producer.head.load(Ordering::Acquire);
            self.consumer.head_cached.set(head);
            if tail == head {
                core::hint::cold_path();
                self.instr.on_pop_empty();
                return slow_pop_empty();
            }
        }

        let value = crate::raw::read_slot(&self.storage, tail);
        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.consumer.tail.store(next_tail, Ordering::Release);
        #[cfg(target_arch = "aarch64")]
        self.consumer.tail_local.set(next_tail);
        self.instr.on_pop();
        Ok(value)
    }

    /// Push a value. Returns `true` on success, `false` if full.
    ///
    /// Unlike `try_push`, drops the value on failure instead of returning it.
    /// Zero overhead — no Result discriminant, no error payload.
    #[inline(always)]
    pub(crate) fn push(&self, value: T) -> bool {
        #[cfg(target_arch = "aarch64")]
        let head = self.producer.head_local.get();
        #[cfg(not(target_arch = "aarch64"))]
        let head = self.producer.head.load(Ordering::Relaxed);

        let next_head = I::wrap(head + 1, self.storage.capacity());

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_write(&self.storage, next_head);

        if next_head == self.producer.tail_cached.get() {
            let tail = self.consumer.tail.load(Ordering::Acquire);
            self.producer.tail_cached.set(tail);
            if next_head == tail {
                core::hint::cold_path();
                self.instr.on_push_full();
                return false;
            }
        }

        crate::raw::write_slot(&self.storage, head, value);
        self.producer.head.store(next_head, Ordering::Release);
        #[cfg(target_arch = "aarch64")]
        self.producer.head_local.set(next_head);
        self.instr.on_push();
        true
    }

    /// Pop into caller's buffer. Returns `true` on success, `false` if empty.
    ///
    /// # Safety
    ///
    /// `out` must be a valid, writeable, properly aligned pointer to `T`.
    #[expect(unsafe_code, reason = "unsafe fn required for raw pointer pop API")]
    #[inline(always)]
    pub(crate) unsafe fn pop(&self, out: *mut T) -> bool {
        #[cfg(target_arch = "aarch64")]
        let tail = self.consumer.tail_local.get();
        #[cfg(not(target_arch = "aarch64"))]
        let tail = self.consumer.tail.load(Ordering::Relaxed);

        #[cfg(feature = "prefetch")]
        crate::raw::prefetch_slot_read(&self.storage, tail);

        if tail == self.consumer.head_cached.get() {
            let head = self.producer.head.load(Ordering::Acquire);
            self.consumer.head_cached.set(head);
            if tail == head {
                core::hint::cold_path();
                self.instr.on_pop_empty();
                return false;
            }
        }

        crate::raw::read_slot_into_unchecked(&self.storage, tail, out);
        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.consumer.tail.store(next_tail, Ordering::Release);
        #[cfg(target_arch = "aarch64")]
        self.consumer.tail_local.set(next_tail);
        self.instr.on_pop();
        true
    }

    pub(crate) fn len(&self) -> usize {
        let head = self.producer.head.load(Ordering::Relaxed);
        let tail = self.consumer.tail.load(Ordering::Relaxed);
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
        let head = self.producer.head.load(Ordering::Relaxed);
        let tail = self.consumer.tail.load(Ordering::Relaxed);
        crate::raw::drop_range::<T, S, I>(&self.storage, tail, head);
    }
}

#[cfg(all(test, not(miri)))]
#[expect(unsafe_code, reason = "tests call unsafe pop API")]
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

    #[test]
    fn push_pop_bool_api() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        assert!(engine.push(42));
        let mut out: u64 = 0;
        assert!(unsafe { engine.pop(&mut out as *mut u64) });
        assert_eq!(out, 42);
    }

    #[test]
    fn push_full_bool_returns_false() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        assert!(engine.push(1));
        assert!(engine.push(2));
        assert!(engine.push(3));
        assert!(!engine.push(4)); // capacity is 4, usable is 3
    }

    #[test]
    fn pop_empty_bool_returns_false() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        let mut out: u64 = 0;
        assert!(!unsafe { engine.pop(&mut out as *mut u64) });
    }

    #[test]
    fn push_pop_bool_fifo() {
        let engine = TestEngine::new(InlineStorage::new(), NoInstr);
        for i in 0..3u64 {
            assert!(engine.push(i));
        }
        for i in 0..3u64 {
            let mut out: u64 = 0;
            assert!(unsafe { engine.pop(&mut out as *mut u64) });
            assert_eq!(out, i);
        }
    }
}
