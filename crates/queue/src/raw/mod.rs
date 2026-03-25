//! Unsafe internals for slot-level operations.
//!
//! All unsafe code in `mantis-queue` lives in this module.
//! The crate root denies unsafe; this module explicitly allows it.
//!
//! Safe wrappers (`write_slot`, `read_slot`, `drop_occupied_slot`,
//! `drop_range`) are provided for use by the engine.

#![allow(unsafe_code)]
// Engine (Task 6) will consume these; suppress until then.
#![allow(dead_code)]

pub(crate) mod slot;

use crate::storage::Storage;
use mantis_core::IndexStrategy;

/// Write a value into a slot.
///
/// The ring engine maintains the invariant that `index` is obtained via
/// `IndexStrategy::wrap` (always < capacity), and the producer
/// exclusively owns slots from `tail_cached..head`.
#[inline]
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
#[inline]
pub(crate) fn read_slot<T, S: Storage<T>>(storage: &S, index: usize) -> T {
    // SAFETY: The ring engine maintains the invariant that `index` is
    // obtained via IndexStrategy::wrap (always < capacity), and the
    // consumer exclusively owns slots from tail..head_cached.
    unsafe { slot::read(storage, index) }
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
