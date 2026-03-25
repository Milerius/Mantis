//! Split producer/consumer handles for the copy-optimized ring.

extern crate alloc;

use alloc::sync::Arc;
use core::marker::PhantomData;

use mantis_core::{CopyPolicy, IndexStrategy, Instrumentation, PushPolicy};

use crate::copy_ring::engine::CopyRingEngine;
use crate::copy_ring::raw::DefaultCopyPolicy;
use crate::storage::{HeapStorage, InlineStorage};
use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

/// Producer handle for the copy-optimized SPSC ring.
pub struct ProducerCopy<T: Copy, S, I, P, Instr, CP> {
    engine: Arc<CopyRingEngine<T, S, I, P, Instr, CP>>,
    _not_sync: PhantomData<*const ()>,
}

/// Consumer handle for the copy-optimized SPSC ring.
pub struct ConsumerCopy<T: Copy, S, I, P, Instr, CP> {
    engine: Arc<CopyRingEngine<T, S, I, P, Instr, CP>>,
    _not_sync: PhantomData<*const ()>,
}

// SAFETY: Producer only accesses head + tail_cached (disjoint from consumer).
// T: Copy + Send ensures the value type is safe to send across threads.
// PhantomData<*const ()> prevents Sync (single-producer discipline).
#[expect(unsafe_code)]
unsafe impl<T, S, I, P, Instr, CP> Send for ProducerCopy<T, S, I, P, Instr, CP>
where
    T: Copy + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
}

// SAFETY: Consumer only accesses tail + head_cached (disjoint from producer).
#[expect(unsafe_code)]
unsafe impl<T, S, I, P, Instr, CP> Send for ConsumerCopy<T, S, I, P, Instr, CP>
where
    T: Copy + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
}

use crate::storage::Storage;

impl<T, S, I, P, Instr, CP> ProducerCopy<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
    /// Push a value by copying it into the ring.
    /// Returns `true` on success, `false` if the ring is full.
    #[inline]
    pub fn push(&self, value: &T) -> bool {
        self.engine.push(value)
    }

    /// Push a batch of values. Returns the number actually pushed.
    #[inline]
    pub fn push_batch(&self, src: &[T]) -> usize {
        self.engine.push_batch(src)
    }
}

impl<T, S, I, P, Instr, CP> ConsumerCopy<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
    /// Pop a value by copying it out of the ring.
    /// Returns `true` on success, `false` if the ring is empty.
    #[inline]
    pub fn pop(&self, out: &mut T) -> bool {
        self.engine.pop(out)
    }

    /// Pop a batch of values. Returns the number actually popped.
    #[inline]
    pub fn pop_batch(&self, dst: &mut [T]) -> usize {
        self.engine.pop_batch(dst)
    }
}

/// Create split producer/consumer handles for an inline copy ring.
#[must_use]
#[expect(clippy::type_complexity)]
pub fn spsc_ring_copy<T: Copy + Send, const N: usize>() -> (
    ProducerCopy<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>,
    ConsumerCopy<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>,
) {
    let engine = Arc::new(CopyRingEngine::new(InlineStorage::new(), NoInstr));
    (
        ProducerCopy { engine: Arc::clone(&engine), _not_sync: PhantomData },
        ConsumerCopy { engine, _not_sync: PhantomData },
    )
}

/// Create split producer/consumer handles for a heap copy ring.
#[must_use]
#[expect(clippy::type_complexity)]
pub fn spsc_ring_copy_heap<T: Copy + Send>(
    capacity: usize,
) -> (
    ProducerCopy<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>,
    ConsumerCopy<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>,
) {
    let engine = Arc::new(CopyRingEngine::new(HeapStorage::new(capacity), NoInstr));
    (
        ProducerCopy { engine: Arc::clone(&engine), _not_sync: PhantomData },
        ConsumerCopy { engine, _not_sync: PhantomData },
    )
}
