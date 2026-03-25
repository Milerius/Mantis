//! Safe split handles for the SPSC ring buffer.

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::sync::Arc;

use core::marker::PhantomData;

use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_types::{PushError, QueueError};

use crate::engine::RingEngine;
use crate::storage::Storage;

#[cfg(feature = "alloc")]
use crate::storage::InlineStorage;

#[cfg(feature = "alloc")]
use crate::storage::HeapStorage;

/// Default SPSC ring type using `Pow2Masked` indexing,
/// `ImmediatePush` policy, and no instrumentation.
#[cfg(feature = "alloc")]
type DefaultProducer<T, S> =
    Producer<T, S, mantis_core::Pow2Masked, mantis_core::ImmediatePush, mantis_core::NoInstr>;

/// Default consumer type matching [`DefaultProducer`].
#[cfg(feature = "alloc")]
type DefaultConsumer<T, S> =
    Consumer<T, S, mantis_core::Pow2Masked, mantis_core::ImmediatePush, mantis_core::NoInstr>;

/// Producer handle -- `Send` but `!Sync`.
///
/// Takes `&mut self` to enforce single-caller discipline,
/// even though the inner engine uses `&self` for its atomics.
#[expect(clippy::type_complexity)]
pub struct Producer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    #[cfg(feature = "alloc")]
    engine: Arc<RingEngine<T, S, I, P, Instr>>,
    _not_sync: PhantomData<*const ()>,
    _types: PhantomData<fn() -> (T, S, I, P, Instr)>,
}

/// Consumer handle -- `Send` but `!Sync`.
///
/// Takes `&mut self` to enforce single-caller discipline,
/// even though the inner engine uses `&self` for its atomics.
#[expect(clippy::type_complexity)]
pub struct Consumer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    #[cfg(feature = "alloc")]
    engine: Arc<RingEngine<T, S, I, P, Instr>>,
    _not_sync: PhantomData<*const ()>,
    _types: PhantomData<fn() -> (T, S, I, P, Instr)>,
}

// SAFETY: Producer only accesses the producer side of the engine
// (head, tail_cached). The SPSC protocol guarantees disjoint access
// between producer and consumer. T: Send is required since values
// cross thread boundaries.
#[expect(unsafe_code)]
unsafe impl<T, S, I, P, Instr> Send for Producer<T, S, I, P, Instr>
where
    T: Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
}

// SAFETY: Consumer only accesses the consumer side of the engine
// (tail, head_cached). The SPSC protocol guarantees disjoint access
// between producer and consumer. T: Send is required since values
// cross thread boundaries.
#[expect(unsafe_code)]
unsafe impl<T, S, I, P, Instr> Send for Consumer<T, S, I, P, Instr>
where
    T: Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
}

#[cfg(feature = "alloc")]
impl<T, S, I, P, Instr> Producer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    /// Push a value into the ring.
    ///
    /// # Errors
    ///
    /// Returns `PushError::Full` if the ring is at capacity.
    #[inline]
    pub fn try_push(&mut self, value: T) -> Result<(), PushError<T>> {
        self.engine.try_push(value)
    }
}

#[cfg(feature = "alloc")]
impl<T, S, I, P, Instr> Consumer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    /// Pop a value from the ring.
    ///
    /// # Errors
    ///
    /// Returns `QueueError::Empty` if the ring has no elements.
    #[inline]
    pub fn try_pop(&mut self) -> Result<T, QueueError> {
        self.engine.try_pop()
    }
}

/// Create an inline SPSC ring, split into producer/consumer.
#[cfg(feature = "alloc")]
#[must_use]
pub fn spsc_ring<T: Send, const N: usize>() -> (
    DefaultProducer<T, InlineStorage<T, N>>,
    DefaultConsumer<T, InlineStorage<T, N>>,
) {
    let engine = Arc::new(RingEngine::new(InlineStorage::new(), mantis_core::NoInstr));
    let tx = Producer {
        engine: Arc::clone(&engine),
        _not_sync: PhantomData,
        _types: PhantomData,
    };
    let rx = Consumer {
        engine,
        _not_sync: PhantomData,
        _types: PhantomData,
    };
    (tx, rx)
}

/// Create a heap SPSC ring, split into producer/consumer.
#[cfg(feature = "alloc")]
#[must_use]
pub fn spsc_ring_heap<T: Send>(
    capacity: usize,
) -> (
    DefaultProducer<T, HeapStorage<T>>,
    DefaultConsumer<T, HeapStorage<T>>,
) {
    let engine = Arc::new(RingEngine::new(
        HeapStorage::new(capacity),
        mantis_core::NoInstr,
    ));
    let tx = Producer {
        engine: Arc::clone(&engine),
        _not_sync: PhantomData,
        _types: PhantomData,
    };
    let rx = Consumer {
        engine,
        _not_sync: PhantomData,
        _types: PhantomData,
    };
    (tx, rx)
}

/// Direct ring access without split handles.
///
/// For single-threaded replay, benchmarking, and power users.
/// Takes `&mut self` on all operations to statically prevent
/// concurrent access.
pub struct RawRing<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    engine: RingEngine<T, S, I, P, Instr>,
}

impl<T, S, I, P, Instr> RawRing<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    /// Create a new `RawRing` with explicit strategy types.
    ///
    /// Prefer the curated preset constructors (`SpscRing::new`,
    /// `SpscRingHeap::with_capacity`, `SpscRingInstrumented::new`)
    /// for common configurations.
    #[must_use]
    pub fn with_strategies(storage: S, instr: Instr) -> Self {
        Self {
            engine: RingEngine::new(storage, instr),
        }
    }

    /// Push a value into the ring.
    ///
    /// # Errors
    ///
    /// Returns `PushError::Full` if the ring is at capacity.
    #[inline]
    pub fn try_push(&mut self, value: T) -> Result<(), PushError<T>> {
        self.engine.try_push(value)
    }

    /// Pop a value from the ring.
    ///
    /// # Errors
    ///
    /// Returns `QueueError::Empty` if the ring has no elements.
    #[inline]
    pub fn try_pop(&mut self) -> Result<T, QueueError> {
        self.engine.try_pop()
    }

    /// Number of elements currently in the ring.
    #[must_use]
    pub fn len(&self) -> usize {
        self.engine.len()
    }

    /// Returns `true` if the ring contains no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.engine.is_empty()
    }

    /// Usable capacity (total slots minus one sentinel).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.engine.capacity()
    }

    /// Access the instrumentation instance.
    #[must_use]
    pub fn instrumentation(&self) -> &Instr {
        self.engine.instrumentation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    #[cfg(feature = "alloc")]
    #[test]
    fn split_handles_push_pop() {
        let (mut tx, mut rx) = spsc_ring::<u64, 4>();
        assert!(tx.try_push(10).is_ok());
        assert_eq!(rx.try_pop().ok(), Some(10));
    }

    #[test]
    fn raw_ring_push_pop() {
        let mut ring =
            RawRing::<u64, InlineStorage<u64, 4>, Pow2Masked, ImmediatePush, NoInstr>::with_strategies(
                InlineStorage::new(),
                NoInstr,
            );
        assert!(ring.try_push(7).is_ok());
        assert_eq!(ring.try_pop().ok(), Some(7));
    }

    #[test]
    fn producer_consumer_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Producer<u64, InlineStorage<u64, 4>, Pow2Masked, ImmediatePush, NoInstr>>();
        assert_send::<Consumer<u64, InlineStorage<u64, 4>, Pow2Masked, ImmediatePush, NoInstr>>();
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn heap_split_handles() {
        let (mut tx, mut rx) = spsc_ring_heap::<u64>(8);
        for i in 0..7 {
            assert!(tx.try_push(i).is_ok());
        }
        for i in 0..7 {
            assert_eq!(rx.try_pop().ok(), Some(i));
        }
    }
}
