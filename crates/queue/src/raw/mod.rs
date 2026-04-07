//! Unsafe internals for slot-level operations.
//!
//! All unsafe code in `mantis-queue` lives in this module.
//! The crate root denies unsafe; this module explicitly allows it.
//!
//! Safe wrappers (`write_slot`, `read_slot`, `drop_occupied_slot`,
//! `drop_range`) are provided for use by the engine.

#![expect(unsafe_code, reason = "raw slot operations require unsafe")]

pub(crate) mod slot;

use crate::storage::Storage;
use mantis_core::IndexStrategy;

/// Write a value into a slot.
///
/// The ring engine maintains the invariant that `index` is obtained via
/// `IndexStrategy::wrap` (always < capacity), and the producer
/// exclusively owns slots from `tail_cached..head`.
#[inline(always)]
pub(crate) fn write_slot<T, S: Storage<T>>(storage: &S, index: usize, value: T) {
    // SAFETY: The ring engine maintains the invariant that `index` is
    // obtained via IndexStrategy::wrap (always < capacity), and the
    // producer exclusively owns slots from tail_cached..head.
    unsafe { slot::write(storage, index, value) }
}

/// Read and move a value out of a slot.
///
/// The ring engine maintains the invariant that `index` is obtained via
/// `IndexStrategy::wrap` (always < capacity), and the consumer
/// exclusively owns slots from `tail..head_cached`.
#[inline(always)]
pub(crate) fn read_slot<T, S: Storage<T>>(storage: &S, index: usize) -> T {
    // SAFETY: The ring engine maintains the invariant that `index` is
    // obtained via IndexStrategy::wrap (always < capacity), and the
    // consumer exclusively owns slots from tail..head_cached.
    unsafe { slot::read(storage, index) }
}

/// Read a slot's value into a caller-provided buffer via raw pointer.
///
/// The ring engine maintains the invariant that `index` is obtained via
/// `IndexStrategy::wrap` (always < capacity), and the consumer
/// exclusively owns slots from `tail..head_cached`.
///
/// **NOT truly safe** — `out` must still be valid. This exists solely so
/// `engine.rs` (which has `deny(unsafe_code)`) can call it; the unsafety
/// is logically pushed to `RingEngine::pop`'s own `unsafe fn` contract.
#[inline(always)]
pub(crate) fn read_slot_into_unchecked<T, S: Storage<T>>(storage: &S, index: usize, out: *mut T) {
    // SAFETY: Caller (RingEngine::pop) guarantees:
    // - index < capacity (obtained via IndexStrategy::wrap)
    // - consumer exclusively owns slot at index
    // - out is valid, writeable, properly aligned (pop's safety contract)
    unsafe { slot::read_into(storage, index, out) }
}

/// Drop a value in a slot during ring teardown.
///
/// During `Drop`, we are the sole owner. Index is obtained from
/// `IndexStrategy::wrap`. Slots between tail..head are initialized.
#[inline]
pub(crate) fn drop_occupied_slot<T, S: Storage<T>>(storage: &S, index: usize) {
    // SAFETY: During Drop, we are the sole owner. Index is obtained
    // from IndexStrategy::wrap. Slots between tail..head are initialized.
    unsafe { slot::drop_slot(storage, index) }
}

/// Drop all occupied slots in a range during ring teardown.
pub(crate) fn drop_range<T, S: Storage<T>, I: IndexStrategy>(
    storage: &S,
    mut tail: usize,
    head: usize,
) {
    let cap = storage.capacity();
    while tail != head {
        drop_occupied_slot(storage, tail);
        tail = I::wrap(tail + 1, cap);
    }
}

/// Prefetch the slot at `index` for writing (producer side).
///
/// No-op when the `prefetch` feature is disabled.
#[cfg(feature = "prefetch")]
#[inline]
pub(crate) fn prefetch_slot_write<T, S: Storage<T>>(storage: &S, index: usize) {
    use mantis_platform::{PrefetchLocality, PrefetchRW, prefetch};
    // SAFETY: index < capacity (guaranteed by IndexStrategy::wrap before this
    // call). slot_ptr returns a valid pointer into owned storage. prefetch is
    // a non-mutating memory hint with no observable side effects.
    let slot = unsafe { storage.slot_ptr(index) };
    prefetch(slot.cast::<u8>(), PrefetchRW::Write, PrefetchLocality::High);
}

/// Prefetch the slot at `index` for reading (consumer side).
///
/// No-op when the `prefetch` feature is disabled.
#[cfg(feature = "prefetch")]
#[inline]
pub(crate) fn prefetch_slot_read<T, S: Storage<T>>(storage: &S, index: usize) {
    use mantis_platform::{PrefetchLocality, PrefetchRW, prefetch};
    // SAFETY: index < capacity (engine invariant: tail is always wrapped via
    // IndexStrategy::wrap before reaching this call). slot_ptr returns a
    // valid pointer. prefetch is a non-mutating hint with no side effects.
    let slot = unsafe { storage.slot_ptr(index) };
    prefetch(slot.cast::<u8>(), PrefetchRW::Read, PrefetchLocality::High);
}

// --- unsafe impl Sync for RingEngine ---
//
// RingEngine contains Cell<usize> (tail_cached, head_cached), which
// makes it !Sync. We need Sync for Arc<RingEngine> in split handles.
//
// SAFETY: The SPSC protocol guarantees disjoint access:
// - ProducerLine (1 cache line): head (AtomicUsize), head_local (aarch64), tail_cached
// - ConsumerLine (1 cache line): tail (AtomicUsize), tail_local (aarch64), head_cached
// - These two sides never touch each other's Cells
// - Atomics are inherently Sync
// - Storage is Sync (required by trait bound)
// The split-handle design (Producer/Consumer) enforces this partition.
// Validated by miri's data-race detection on every PR.
use crate::engine::RingEngine;
use mantis_core::{Instrumentation, PushPolicy};

unsafe impl<T, S, I, P, Instr> Sync for RingEngine<T, S, I, P, Instr>
where
    T: Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation + Sync,
{
}
