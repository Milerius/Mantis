//! Low-level slot read/write operations on `MaybeUninit<T>`.
//!
//! These functions operate on raw pointers obtained from a `Storage`
//! implementation. Callers must uphold the single-writer invariant.

#![expect(unsafe_code, reason = "MaybeUninit slot operations require unsafe")]

use core::ptr;

use crate::storage::Storage;

/// Write `value` into the slot at `index`.
///
/// # Safety
///
/// - `index` must be less than `storage.capacity()`
/// - The slot at `index` must be logically empty
/// - No other thread may concurrently access this slot
#[inline]
pub(crate) unsafe fn write<T, S: Storage<T>>(storage: &S, index: usize, value: T) {
    // SAFETY: Caller guarantees index < capacity and exclusive access.
    let slot = unsafe { storage.slot_ptr(index) };
    // SAFETY: Caller guarantees the slot is logically empty, so
    // overwriting is safe. Pointer is valid per Storage contract.
    unsafe { ptr::write((*slot).as_mut_ptr(), value) };
}

/// Read and move the value out of the slot at `index`.
///
/// # Safety
///
/// - `index` must be less than `storage.capacity()`
/// - The slot must contain a valid, initialized value
/// - After this call the slot is logically empty
/// - No other thread may concurrently access this slot
#[inline]
pub(crate) unsafe fn read<T, S: Storage<T>>(storage: &S, index: usize) -> T {
    // SAFETY: Caller guarantees index < capacity and exclusive access.
    let slot = unsafe { storage.slot_ptr(index) };
    // SAFETY: Caller guarantees the slot contains an initialized value.
    unsafe { ptr::read((*slot).as_ptr()) }
}

/// Drop the value in the slot at `index` in place.
///
/// # Safety
///
/// - `index` must be less than `storage.capacity()`
/// - The slot must contain a valid, initialized value
/// - After this call the slot is logically empty
/// - No other thread may concurrently access this slot
#[inline]
pub(crate) unsafe fn drop_slot<T, S: Storage<T>>(storage: &S, index: usize) {
    // SAFETY: Caller guarantees index < capacity and exclusive access.
    let slot = unsafe { storage.slot_ptr(index) };
    // SAFETY: Caller guarantees the slot contains an initialized value.
    // After this call the slot is logically empty.
    unsafe { ptr::drop_in_place((*slot).as_mut_ptr()) };
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::string::String;

    use super::*;
    use crate::InlineStorage;

    #[test]
    fn write_then_read_u64() {
        let storage = InlineStorage::<u64, 4>::new();
        // SAFETY: index 0 < capacity 4, slot is uninitialized,
        // single-threaded test.
        unsafe {
            write(&storage, 0, 42u64);
            let val = read(&storage, 0);
            assert_eq!(val, 42);
        }
    }

    #[test]
    fn write_then_read_string() {
        let storage = InlineStorage::<String, 4>::new();
        // SAFETY: index 1 < capacity 4, slot is uninitialized,
        // single-threaded test.
        unsafe {
            write(&storage, 1, String::from("hello"));
            let val = read(&storage, 1);
            assert_eq!(val, "hello");
        }
    }

    #[test]
    fn drop_in_place_runs_destructor() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct DropCounter;
        impl Drop for DropCounter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        DROP_COUNT.store(0, Ordering::Relaxed);
        let storage = InlineStorage::<DropCounter, 4>::new();
        // SAFETY: index 0 < capacity 4, slot is uninitialized,
        // single-threaded test.
        unsafe {
            write(&storage, 0, DropCounter);
            drop_slot(&storage, 0);
        }
        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }
}
