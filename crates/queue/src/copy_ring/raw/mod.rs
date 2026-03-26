//! Unsafe internals for copy-ring slot operations.
//!
//! All unsafe code for `copy_ring` lives here. The crate root denies
//! unsafe; this module explicitly allows it.
//!
//! Safe wrappers (`write_slot_copy`, `read_slot_copy`) are provided for
//! use by the copy-ring engine.

#![expect(unsafe_code, reason = "raw slot access requires unsafe")]

use core::ptr;

use mantis_platform::CopyPolicy;

use crate::storage::Storage;

/// Write a value into a slot using the provided copy policy.
///
/// The ring engine maintains the invariant that `index` is always
/// `< storage.capacity()`, and the producer exclusively owns the target
/// slot at the time of the call (SPSC protocol).
#[inline]
pub(crate) fn write_slot_copy<T: Copy, S: Storage<T>, CP: CopyPolicy<T>>(
    storage: &S,
    index: usize,
    value: &T,
) {
    // SAFETY: `index < capacity` is guaranteed by the caller (ring engine
    // uses IndexStrategy::wrap). The slot is logically unoccupied: the
    // producer owns slots from tail_cached..head exclusively under SPSC.
    // slot_ptr returns a *mut MaybeUninit<T>; casting to *mut T is valid
    // because T: Copy has no drop glue and Storage guarantees alignment.
    // ptr::addr_of!(*value) avoids creating an intermediate reference.
    unsafe {
        let dst = storage.slot_ptr(index).cast::<T>();
        CP::copy_in(dst, ptr::addr_of!(*value));
    }
}

/// Read a value from a slot using the provided copy policy.
///
/// The ring engine maintains the invariant that `index` is always
/// `< storage.capacity()`, and the consumer exclusively owns the source
/// slot at the time of the call (SPSC protocol).
#[inline]
pub(crate) fn read_slot_copy<T: Copy, S: Storage<T>, CP: CopyPolicy<T>>(
    storage: &S,
    index: usize,
    out: &mut T,
) {
    // SAFETY: `index < capacity` is guaranteed by the caller. The slot is
    // logically occupied and initialized: the consumer owns slots from
    // tail..head_cached exclusively under SPSC, and the producer has
    // previously written via write_slot_copy. Casting *mut MaybeUninit<T>
    // to *const T is valid because the slot is initialized and T: Copy.
    unsafe {
        let src = storage.slot_ptr(index).cast::<T>();
        CP::copy_out(ptr::addr_of_mut!(*out), src);
    }
}

/// Copy `src.len()` contiguous values from `src` into the ring's backing
/// array starting at `start_index`.
///
/// Used by `push_batch` for the two-chunk contiguous write path.
/// The caller must guarantee:
/// - `start_index + src.len() <= storage.capacity()` (no wrapping).
/// - The producer exclusively owns all slots in the range (SPSC invariant).
///
/// This is a safe wrapper: all safety requirements are upheld structurally
/// by the call sites in `engine.rs` via SPSC protocol and index arithmetic.
#[inline]
pub(crate) fn write_batch_copy<T: Copy, S: Storage<T>>(storage: &S, start_index: usize, src: &[T]) {
    let count = src.len();
    if count == 0 {
        return;
    }
    // SAFETY:
    // - `base_ptr()` returns a pointer with provenance over all `capacity()`
    //   slots (required by the `Storage` trait contract).
    // - `start_index + count <= capacity` is guaranteed by the caller, so
    //   `base_ptr().add(start_index)` through `+count` is in-bounds.
    // - `MaybeUninit<T>` has the same size, alignment, and ABI as `T`
    //   (stdlib guarantee), so casting `*mut MaybeUninit<T>` to `*mut T`
    //   and writing `count` elements is valid.
    // - The producer exclusively owns these slots under the SPSC protocol;
    //   head is published with Release only after this write completes,
    //   so no concurrent reader can observe uninitialized data.
    unsafe {
        let dst = storage.base_ptr().add(start_index).cast::<T>();
        ptr::copy_nonoverlapping(src.as_ptr(), dst, count);
    }
}

// --- unsafe impl Sync for CopyRingEngine ---
//
// CopyRingEngine contains Cell<usize> (tail_cached, head_cached), which
// makes it !Sync. We need Sync for Arc<CopyRingEngine> in split handles.
//
// SAFETY: The SPSC protocol guarantees disjoint access:
// - Producer ONLY accesses: head (AtomicUsize), tail_cached (Cell<usize>)
// - Consumer ONLY accesses: tail (AtomicUsize), head_cached (Cell<usize>)
// These two sides never touch each other's Cell. Atomics are inherently
// Sync. Storage is Sync (required by trait bound). The split-handle design
// enforces this partition at compile time. Validated by Miri on every PR.
use crate::copy_ring::engine::CopyRingEngine;
use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};

unsafe impl<T, S, I, P, Instr, CP> Sync for CopyRingEngine<T, S, I, P, Instr, CP>
where
    T: Copy + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation + Sync,
    CP: CopyPolicy<T>,
{
}
