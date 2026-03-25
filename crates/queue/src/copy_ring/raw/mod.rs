//! Unsafe internals for copy-ring slot operations.
//!
//! All unsafe code for `copy_ring` lives here. The crate root denies
//! unsafe; this module explicitly allows it.
//!
//! Safe wrappers (`write_slot_copy`, `read_slot_copy`) are provided for
//! use by the copy-ring engine.

#![expect(unsafe_code, reason = "raw slot access requires unsafe")]
// write_slot_copy, read_slot_copy, and DefaultCopyPolicy are consumed by
// the copy-ring engine added in a later task (Task 3+).
#![expect(
    dead_code,
    unused_imports,
    reason = "consumed by copy-ring engine added in Task 3+"
)]

pub(crate) mod simd;
// Re-exported for use by the copy-ring engine (added in a later task).
pub(crate) use simd::DefaultCopyPolicy;
#[cfg(feature = "nightly")]
pub(crate) use simd::SimdCopyPolicy;

use core::ptr;

use mantis_core::CopyPolicy;

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
