//! Storage backends for ring buffer slot arrays.

#![allow(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::boxed::Box;

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

use mantis_types::AssertPowerOfTwo;

/// Backing storage for a ring buffer.
///
/// # Safety
///
/// Implementors must guarantee that `slot_ptr` returns a valid,
/// aligned pointer for any `index < capacity()`.
pub unsafe trait Storage<T>: Send + Sync {
    /// Number of usable slots.
    fn capacity(&self) -> usize;

    /// Pointer to the slot at `index`.
    ///
    /// # Safety
    ///
    /// Caller must ensure `index < capacity()`.
    unsafe fn slot_ptr(&self, index: usize) -> *mut MaybeUninit<T>;
}

/// Stack-allocated, const-generic storage. `N` must be a power of 2.
pub struct InlineStorage<T, const N: usize> {
    slots: [UnsafeCell<MaybeUninit<T>>; N],
}

impl<T, const N: usize> InlineStorage<T, N> {
    /// Create new inline storage with uninitialized slots.
    ///
    /// Compile-time assertion ensures `N` is a power of two.
    #[must_use]
    pub fn new() -> Self {
        const { AssertPowerOfTwo::<N>::VALID };
        Self {
            // SAFETY: MaybeUninit does not require initialization.
            slots: core::array::from_fn(|_| UnsafeCell::new(MaybeUninit::uninit())),
        }
    }
}

impl<T, const N: usize> Default for InlineStorage<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: Slots are accessed through raw pointers with single-writer
// discipline enforced by the engine's split-handle design.
unsafe impl<T: Send, const N: usize> Send for InlineStorage<T, N> {}
// SAFETY: The engine enforces that each slot is accessed by at most one
// thread at a time (producer writes, consumer reads, never concurrently
// on the same slot).
unsafe impl<T: Send, const N: usize> Sync for InlineStorage<T, N> {}

unsafe impl<T: Send, const N: usize> Storage<T> for InlineStorage<T, N> {
    #[inline]
    fn capacity(&self) -> usize {
        N
    }

    #[inline]
    unsafe fn slot_ptr(&self, index: usize) -> *mut MaybeUninit<T> {
        debug_assert!(index < N, "slot index {index} out of bounds (capacity {N})");
        // SAFETY: Caller guarantees index < N. UnsafeCell::get returns
        // a raw pointer to the inner MaybeUninit.
        unsafe { self.slots.get_unchecked(index).get() }
    }
}

/// Heap-allocated storage with runtime-sized capacity.
///
/// Capacity is rounded up to the next power of 2 at construction.
#[cfg(feature = "alloc")]
pub struct HeapStorage<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    capacity: usize,
}

#[cfg(feature = "alloc")]
impl<T> HeapStorage<T> {
    /// Create heap storage with at least `min_capacity` slots.
    ///
    /// Actual capacity is rounded up to the next power of two.
    ///
    /// # Panics
    ///
    /// Panics if `min_capacity` is zero.
    #[must_use]
    pub fn new(min_capacity: usize) -> Self {
        assert!(min_capacity > 0, "capacity must be non-zero");
        let capacity = min_capacity.next_power_of_two();
        let slots: Box<[_]> = (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect();
        Self { slots, capacity }
    }
}

#[cfg(feature = "alloc")]
unsafe impl<T: Send> Send for HeapStorage<T> {}
#[cfg(feature = "alloc")]
unsafe impl<T: Send> Sync for HeapStorage<T> {}

#[cfg(feature = "alloc")]
unsafe impl<T: Send> Storage<T> for HeapStorage<T> {
    #[inline]
    fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    unsafe fn slot_ptr(&self, index: usize) -> *mut MaybeUninit<T> {
        debug_assert!(
            index < self.capacity,
            "slot index {index} out of bounds (capacity {})",
            self.capacity
        );
        // SAFETY: Caller guarantees index < capacity.
        unsafe { self.slots.get_unchecked(index).get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_storage_capacity() {
        let storage = InlineStorage::<u64, 8>::new();
        assert_eq!(storage.capacity(), 8);
    }

    #[test]
    fn inline_storage_slot_roundtrip() {
        let storage = InlineStorage::<u64, 4>::new();
        unsafe {
            let ptr = storage.slot_ptr(0);
            (*ptr).write(42u64);
            let val = (*ptr).assume_init_read();
            assert_eq!(val, 42);
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn heap_storage_rounds_up_capacity() {
        let storage = HeapStorage::<u64>::new(5);
        assert_eq!(storage.capacity(), 8);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn heap_storage_slot_roundtrip() {
        let storage = HeapStorage::<u64>::new(4);
        unsafe {
            let ptr = storage.slot_ptr(0);
            (*ptr).write(99u64);
            let val = (*ptr).assume_init_read();
            assert_eq!(val, 99);
        }
    }
}
