# SPSC Ring Buffer — Implementation Plan (1 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a lock-free SPSC ring buffer in `mantis-queue` with modular strategy pattern, split handles, and 3 presets (`SpscRing`, `SpscRingHeap`, `SpscRingInstrumented`).

**Architecture:** Generic `RingEngine<T, S, I, P, Instr>` parameterized by storage, index, push policy, and instrumentation strategies. Unsafe code isolated in `raw/` submodule. Safe `Producer`/`Consumer` split handles enforce SPSC at compile time. `CachePadded<T>` prevents false sharing with 128-byte alignment.

**Tech Stack:** Rust no_std, `core::sync::atomic` (Acquire/Release), `MaybeUninit<T>`, `UnsafeCell`, `Cell` for cached indices, `alloc` for heap storage.

**Spec:** `docs/specs/2026-03-25-spsc-ring-bench-design.md`

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `crates/queue/src/pad.rs` | `CachePadded<T>` wrapper (`#[repr(align(128))]`) |
| `crates/queue/src/storage.rs` | `Storage<T>` trait, `InlineStorage<T, N>`, `HeapStorage<T>` |
| `crates/queue/src/raw/mod.rs` | `#[allow(unsafe_code)]` re-exports |
| `crates/queue/src/raw/slot.rs` | `write`, `read`, `drop_in_place` — all unsafe slot ops |
| `crates/queue/src/engine.rs` | `RingEngine<T, S, I, P, Instr>` — core push/pop logic |
| `crates/queue/src/handle.rs` | `Producer<T, ...>`, `Consumer<T, ...>`, `RawRing<T, ...>` |
| `crates/queue/src/presets.rs` | Type aliases: `SpscRing`, `SpscRingHeap`, `SpscRingInstrumented` |
| `crates/queue/tests/spsc_basic.rs` | Integration tests for all presets |
| `crates/queue/tests/spsc_stress.rs` | Two-thread 10M-item stress test |

### Modified files

| File | Changes |
|---|---|
| `crates/types/src/lib.rs` | Add `PushError<T>` enum |
| `crates/core/src/lib.rs` | Add `CountingInstr` struct |
| `crates/queue/src/lib.rs` | Add modules, re-exports, presets |
| `crates/queue/Cargo.toml` | Add `asm` and `alloc` features |
| `crates/core/Cargo.toml` | No changes needed (already no_std with atomics) |

---

## Task 1: Add `PushError<T>` to `mantis-types`

**Files:**
- Modify: `crates/types/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/types/src/lib.rs` in the `tests` module:

```rust
#[test]
fn push_error_preserves_value() {
    let err = PushError::Full(42u64);
    match err {
        PushError::Full(v) => assert_eq!(v, 42),
    }
}

#[test]
fn push_error_display() {
    let err = PushError::Full(0u32);
    assert_eq!(err.to_string(), "queue is full");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mantis-types`
Expected: FAIL — `PushError` not found

- [ ] **Step 3: Implement `PushError<T>`**

Add above the `QueueError` definition in `crates/types/src/lib.rs`:

```rust
/// Error returned when pushing to a full queue, preserving the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError<T> {
    /// The queue is full. Contains the value that was not pushed.
    Full(T),
}

impl<T> fmt::Display for PushError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(_) => write!(f, "queue is full"),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mantis-types`
Expected: PASS — all 6 tests pass

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p mantis-types --all-targets -- -D warnings`
Expected: No warnings

- [ ] **Step 6: Commit**

```bash
git add crates/types/src/lib.rs
git commit -m "feat(types): add PushError<T> preserving value on full"
```

---

## Task 2: Add `CountingInstr` to `mantis-core`

**Files:**
- Modify: `crates/core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/core/src/lib.rs` (add `#[cfg(test)] mod tests` block):

```rust
#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    #[test]
    fn counting_instr_tracks_push_pop() {
        let instr = CountingInstr::new();
        instr.on_push();
        instr.on_push();
        instr.on_pop();
        instr.on_push_full();
        instr.on_pop_empty();
        instr.on_pop_empty();
        assert_eq!(instr.push_count(), 2);
        assert_eq!(instr.pop_count(), 1);
        assert_eq!(instr.push_full_count(), 1);
        assert_eq!(instr.pop_empty_count(), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mantis-core`
Expected: FAIL — `CountingInstr` not found

- [ ] **Step 3: Implement `CountingInstr`**

Add to `crates/core/src/lib.rs` after `NoInstr`:

```rust
use core::sync::atomic::{AtomicU64, Ordering};

/// Instrumentation that counts push/pop operations via atomic counters.
///
/// All increments use `Relaxed` ordering (counters are advisory, not
/// synchronization primitives). Suitable for debug/profiling presets.
pub struct CountingInstr {
    pushes: AtomicU64,
    pops: AtomicU64,
    push_full: AtomicU64,
    pop_empty: AtomicU64,
}

impl CountingInstr {
    /// Create a new counter with all values at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pushes: AtomicU64::new(0),
            pops: AtomicU64::new(0),
            push_full: AtomicU64::new(0),
            pop_empty: AtomicU64::new(0),
        }
    }

    /// Total successful pushes.
    #[must_use]
    pub fn push_count(&self) -> u64 {
        self.pushes.load(Ordering::Relaxed)
    }

    /// Total successful pops.
    #[must_use]
    pub fn pop_count(&self) -> u64 {
        self.pops.load(Ordering::Relaxed)
    }

    /// Total push attempts that failed (queue full).
    #[must_use]
    pub fn push_full_count(&self) -> u64 {
        self.push_full.load(Ordering::Relaxed)
    }

    /// Total pop attempts that failed (queue empty).
    #[must_use]
    pub fn pop_empty_count(&self) -> u64 {
        self.pop_empty.load(Ordering::Relaxed)
    }
}

impl Instrumentation for CountingInstr {
    #[inline]
    fn on_push(&self) {
        self.pushes.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_pop(&self) {
        self.pops.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_push_full(&self) {
        self.push_full.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_pop_empty(&self) {
        self.pop_empty.fetch_add(1, Ordering::Relaxed);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p mantis-core`
Expected: PASS

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p mantis-core --all-targets -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/lib.rs
git commit -m "feat(core): add CountingInstr with atomic push/pop counters"
```

---

## Task 3: `CachePadded<T>` wrapper

**Files:**
- Create: `crates/queue/src/pad.rs`

- [ ] **Step 1: Write the test**

In `crates/queue/src/pad.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_padded_alignment() {
        assert_eq!(core::mem::align_of::<CachePadded<u64>>(), 128);
    }

    #[test]
    fn cache_padded_deref() {
        let padded = CachePadded::new(42u64);
        assert_eq!(*padded, 42);
    }

    #[test]
    fn cache_padded_deref_mut() {
        let mut padded = CachePadded::new(42u64);
        *padded = 99;
        assert_eq!(*padded, 99);
    }
}
```

- [ ] **Step 2: Implement `CachePadded<T>`**

```rust
//! Cache-line padding to prevent false sharing.

use core::ops::{Deref, DerefMut};

/// Aligns the inner value to 128 bytes, covering both Intel (64B)
/// and Apple Silicon (128B) cache lines. Prevents false sharing
/// between adjacent atomics in the ring engine.
#[derive(Debug)]
#[repr(align(128))]
pub struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    /// Wrap a value with cache-line padding.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for CachePadded<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}
```

- [ ] **Step 3: Wire up module in `lib.rs`**

Add `mod pad;` and `pub use pad::CachePadded;` to `crates/queue/src/lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-queue`
Expected: PASS — 3 new tests

- [ ] **Step 5: Commit**

```bash
git add crates/queue/src/pad.rs crates/queue/src/lib.rs
git commit -m "feat(queue): add CachePadded<T> with 128-byte alignment"
```

---

## Task 4: `Storage<T>` trait + `InlineStorage` + `HeapStorage`

**Files:**
- Create: `crates/queue/src/storage.rs`
- Modify: `crates/queue/Cargo.toml` (add `alloc` feature)
- Modify: `crates/queue/src/lib.rs` (add module)

- [ ] **Step 1: Add `alloc` feature to Cargo.toml**

In `crates/queue/Cargo.toml`, update features:

```toml
[features]
default = []
std = ["alloc"]
alloc = []
asm = []
```

- [ ] **Step 2: Write failing tests for `InlineStorage`**

In `crates/queue/src/storage.rs`:

```rust
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
}
```

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p mantis-queue`
Expected: FAIL — `Storage`, `InlineStorage` not found

- [ ] **Step 4: Implement `Storage` trait and `InlineStorage`**

```rust
//! Storage backends for ring buffer slot arrays.

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
        self.slots.get_unchecked(index).get()
    }
}
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p mantis-queue`
Expected: PASS

- [ ] **Step 6: Add `HeapStorage` behind `alloc` feature**

Add to `crates/queue/src/storage.rs`:

```rust
#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::boxed::Box;

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
        self.slots.get_unchecked(index).get()
    }
}
```

- [ ] **Step 7: Add `HeapStorage` tests**

```rust
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
```

- [ ] **Step 8: Wire up module and run all tests**

Add `mod storage; pub use storage::*;` to lib.rs.

Run: `cargo test -p mantis-queue --all-features`
Expected: PASS — all storage tests pass

- [ ] **Step 9: Run clippy**

Run: `cargo clippy -p mantis-queue --all-targets --all-features -- -D warnings`

- [ ] **Step 10: Commit**

```bash
git add crates/queue/src/storage.rs crates/queue/src/lib.rs crates/queue/Cargo.toml
git commit -m "feat(queue): add Storage trait, InlineStorage, HeapStorage"
```

---

## Task 5: `raw` module — unsafe slot operations + safe engine wrappers

**Files:**
- Create: `crates/queue/src/raw/mod.rs`
- Create: `crates/queue/src/raw/slot.rs`
- Modify: `crates/queue/src/lib.rs`

**Design decision:** Per `docs/UNSAFE.md`, all unsafe code lives in `raw` submodules. The `raw::slot` functions are `unsafe fn`, which means callers need `unsafe` blocks. To keep `engine.rs` free of `unsafe`, we expose safe wrapper functions from `raw/mod.rs` that encapsulate the unsafe calls with documented safety contracts. The engine calls these safe wrappers.

- [ ] **Step 1: Write failing tests in `raw/slot.rs`**

```rust
#[cfg(test)]
mod tests {
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
        unsafe {
            write(&storage, 0, DropCounter);
            drop_slot(&storage, 0);
        }
        assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p mantis-queue --all-features`
Expected: FAIL — `raw` module not found

- [ ] **Step 3: Implement `raw/mod.rs`**

This module contains unsafe code and also provides safe wrappers that `engine.rs` calls. The safe wrappers document why their usage is sound, centralizing the safety argument here rather than scattering `#[allow(unsafe_code)]` through the engine.

```rust
//! Unsafe internals for slot-level operations.
//!
//! All unsafe code in `mantis-queue` lives in this module.
//! The crate root denies unsafe; this module explicitly allows it.
//!
//! Safe wrappers (`write_slot`, `read_slot`, `drop_occupied_slot`,
//! `drop_range`) are provided for use by the engine. These wrappers
//! are safe to call when the engine's invariants hold:
//! - Index is obtained from `IndexStrategy::wrap` on a value < 2*capacity
//! - Slot ownership follows the ring protocol (producer owns
//!   tail_cached..head, consumer owns head_cached..tail)
//! - No concurrent access to the same slot

#![allow(unsafe_code)]

pub(crate) mod slot;

use crate::storage::Storage;
use mantis_core::IndexStrategy;

/// Write a value into a slot. The engine guarantees:
/// - `index < storage.capacity()` (from IndexStrategy::wrap)
/// - The slot is logically empty (producer owns it)
/// - No concurrent access (single-producer)
#[inline]
pub(crate) fn write_slot<T, S: Storage<T>>(
    storage: &S,
    index: usize,
    value: T,
) {
    // SAFETY: The ring engine maintains the invariant that `index` is
    // obtained via IndexStrategy::wrap (always < capacity), and the
    // producer exclusively owns slots from tail_cached..head.
    unsafe { slot::write(storage, index, value) }
}

/// Read and move a value out of a slot. The engine guarantees:
/// - `index < storage.capacity()` (from IndexStrategy::wrap)
/// - The slot contains a valid value (consumer owns it)
/// - No concurrent access (single-consumer)
#[inline]
pub(crate) fn read_slot<T, S: Storage<T>>(
    storage: &S,
    index: usize,
) -> T {
    // SAFETY: The ring engine maintains the invariant that `index` is
    // obtained via IndexStrategy::wrap (always < capacity), and the
    // consumer exclusively owns slots from tail..head_cached.
    unsafe { slot::read(storage, index) }
}

/// Drop a value in a slot during ring teardown. Called only from
/// the Drop impl when the ring is the sole owner.
#[inline]
pub(crate) fn drop_occupied_slot<T, S: Storage<T>>(
    storage: &S,
    index: usize,
) {
    // SAFETY: During Drop, we are the sole owner. Index is obtained
    // from IndexStrategy::wrap. Slots between tail..head are
    // guaranteed initialized.
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

// --- unsafe impl Sync for RingEngine ---
//
// RingEngine contains Cell<usize> (tail_cached, head_cached), which
// makes it !Sync. We need Sync for Arc<RingEngine> in split handles.
//
// SAFETY: The SPSC protocol guarantees disjoint access:
// - Producer ONLY accesses: head (AtomicUsize), tail_cached (Cell)
// - Consumer ONLY accesses: tail (AtomicUsize), head_cached (Cell)
// - These two sides never touch each other's Cell
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
{}
```

- [ ] **Step 4: Implement `raw/slot.rs`**

```rust
//! Low-level slot read/write operations on `MaybeUninit<T>`.
//!
//! These functions operate on raw pointers obtained from a `Storage`
//! implementation. Callers must uphold the single-writer invariant:
//! only one thread writes a given slot, and only one thread reads it,
//! and a slot is not read and written concurrently.

use core::ptr;

use crate::storage::Storage;

/// Write `value` into the slot at `index`.
///
/// # Safety
///
/// - `index` must be less than `storage.capacity()`
/// - The slot at `index` must be logically empty (not yet written, or
///   previously read). Writing to an occupied slot of a `Drop` type
///   will leak the old value.
/// - No other thread may concurrently access this slot.
#[inline]
pub(crate) unsafe fn write<T, S: Storage<T>>(
    storage: &S,
    index: usize,
    value: T,
) {
    // SAFETY: Caller guarantees index < capacity and exclusive access.
    // We write into an uninitialized MaybeUninit slot.
    let slot = storage.slot_ptr(index);
    ptr::write((*slot).as_mut_ptr(), value);
}

/// Read and move the value out of the slot at `index`.
///
/// # Safety
///
/// - `index` must be less than `storage.capacity()`
/// - The slot at `index` must contain a valid, initialized value.
/// - After this call the slot is logically empty — the caller must
///   not read it again without a preceding write.
/// - No other thread may concurrently access this slot.
#[inline]
pub(crate) unsafe fn read<T, S: Storage<T>>(
    storage: &S,
    index: usize,
) -> T {
    // SAFETY: Caller guarantees index < capacity, slot is initialized,
    // and exclusive access. ptr::read moves the value out.
    let slot = storage.slot_ptr(index);
    ptr::read((*slot).as_ptr())
}

/// Drop the value in the slot at `index` in place.
///
/// # Safety
///
/// - `index` must be less than `storage.capacity()`
/// - The slot at `index` must contain a valid, initialized value.
/// - After this call the slot is logically empty.
/// - No other thread may concurrently access this slot.
#[inline]
pub(crate) unsafe fn drop_slot<T, S: Storage<T>>(
    storage: &S,
    index: usize,
) {
    // SAFETY: Caller guarantees index < capacity, slot is initialized.
    // We drop the value in place, leaving the slot uninitialized.
    let slot = storage.slot_ptr(index);
    ptr::drop_in_place((*slot).as_mut_ptr());
}
```

- [ ] **Step 5: Wire up module in `lib.rs`**

Add to `crates/queue/src/lib.rs`:

```rust
mod raw;
```

Note: `raw` is private — not public API. The `#![allow(unsafe_code)]` is on `raw/mod.rs`, keeping `engine.rs` and the rest of the crate `deny(unsafe_code)`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: PASS — all slot tests pass

- [ ] **Step 7: Run clippy + miri**

Run:
```bash
cargo clippy -p mantis-queue --all-targets --all-features -- -D warnings
cargo +nightly miri test -p mantis-queue --all-features
```

- [ ] **Step 8: Commit**

```bash
git add crates/queue/src/raw/
git commit -m "feat(queue): add raw::slot unsafe ops + safe engine wrappers"
```

---

## Task 6: `RingEngine` core

**Files:**
- Create: `crates/queue/src/engine.rs`
- Modify: `crates/queue/src/lib.rs`

- [ ] **Step 1: Write failing tests in `engine.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    type TestEngine = RingEngine<
        u64,
        InlineStorage<u64, 4>,
        Pow2Masked,
        ImmediatePush,
        NoInstr,
    >;

    #[test]
    fn push_pop_single() {
        let engine = TestEngine::new(
            InlineStorage::new(),
            NoInstr,
        );
        assert!(engine.try_push(42).is_ok());
        assert_eq!(engine.try_pop().ok(), Some(42));
    }

    #[test]
    fn push_full_returns_value() {
        let engine = TestEngine::new(
            InlineStorage::new(),
            NoInstr,
        );
        // Fill all 4 slots (capacity 4 means 3 usable + 1 sentinel,
        // but with power-of-2 masking the full condition is
        // next_head == tail, so we can store capacity-1 = 3 items)
        assert!(engine.try_push(1).is_ok());
        assert!(engine.try_push(2).is_ok());
        assert!(engine.try_push(3).is_ok());
        let err = engine.try_push(4);
        assert_eq!(err, Err(PushError::Full(4)));
    }

    #[test]
    fn pop_empty_returns_error() {
        let engine = TestEngine::new(
            InlineStorage::new(),
            NoInstr,
        );
        assert_eq!(engine.try_pop(), Err(QueueError::Empty));
    }

    #[test]
    fn fifo_ordering() {
        let engine = TestEngine::new(
            InlineStorage::new(),
            NoInstr,
        );
        for i in 0..3 {
            assert!(engine.try_push(i).is_ok());
        }
        for i in 0..3 {
            assert_eq!(engine.try_pop().ok(), Some(i));
        }
    }

    #[test]
    fn wraparound() {
        let engine = TestEngine::new(
            InlineStorage::new(),
            NoInstr,
        );
        // Push 3, pop 3, push 3 more (forces wraparound on capacity 4)
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
        let engine = TestEngine::new(
            InlineStorage::new(),
            NoInstr,
        );
        assert!(engine.is_empty());
        assert_eq!(engine.len(), 0);
        assert!(engine.try_push(1).is_ok());
        assert!(!engine.is_empty());
        assert_eq!(engine.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p mantis-queue --all-features`
Expected: FAIL — `RingEngine` not found

- [ ] **Step 3: Implement `RingEngine`**

In `crates/queue/src/engine.rs`:

```rust
//! Core SPSC ring buffer engine.
//!
//! `RingEngine` is the internal, non-public engine that implements the
//! Acquire/Release ring buffer protocol with cached remote indices.
//! It is parameterized by strategy traits and exposed through safe
//! public handles (`Producer`/`Consumer`) or `RawRing`.

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_types::{PushError, QueueError};

use crate::pad::CachePadded;
use crate::storage::Storage;

/// Generic SPSC ring engine. Not public — use `Producer`/`Consumer`
/// or `RawRing` instead.
///
/// Contains `Cell<usize>` fields for cached indices, making it `!Sync`
/// by default. We provide `unsafe impl Sync` because the SPSC protocol
/// guarantees disjoint access: the producer only touches `head` +
/// `tail_cached`, and the consumer only touches `tail` + `head_cached`.
pub(crate) struct RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    /// Head index — written by producer, read by consumer.
    head: CachePadded<AtomicUsize>,
    /// Tail index — written by consumer, read by producer.
    tail: CachePadded<AtomicUsize>,
    /// Producer's cached copy of tail (avoids cross-core read).
    tail_cached: CachePadded<Cell<usize>>,
    /// Consumer's cached copy of head (avoids cross-core read).
    head_cached: CachePadded<Cell<usize>>,
    /// Slot storage backend.
    storage: S,
    /// Instrumentation hooks.
    instr: Instr,
    _marker: PhantomData<(T, I, P)>,
}

// NOTE: `unsafe impl Sync` for RingEngine lives in `raw/mod.rs`
// (per unsafe isolation policy). See that module for the safety argument.

impl<T, S, I, P, Instr> RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    /// Create a new ring engine with the given storage and
    /// instrumentation.
    pub(crate) fn new(storage: S, instr: Instr) -> Self {
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            tail_cached: CachePadded::new(Cell::new(0)),
            head_cached: CachePadded::new(Cell::new(0)),
            storage,
            instr,
            _marker: PhantomData,
        }
    }

    /// Usable capacity (one slot reserved as sentinel).
    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity() - 1
    }

    /// Try to push a value. Returns `Err(PushError::Full(value))` if
    /// full.
    ///
    /// Only the producer side should call this.
    #[inline]
    pub(crate) fn try_push(&self, value: T) -> Result<(), PushError<T>> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = I::wrap(head + 1, self.storage.capacity());

        if next_head == self.tail_cached.get() {
            // Cache miss — reload tail from the consumer.
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            if next_head == tail {
                self.instr.on_push_full();
                return Err(PushError::Full(value));
            }
        }

        crate::raw::write_slot(&self.storage, head, value);

        self.head.store(next_head, Ordering::Release);
        self.instr.on_push();
        Ok(())
    }

    /// Try to pop a value. Returns `Err(QueueError::Empty)` if empty.
    ///
    /// Only the consumer side should call this.
    #[inline]
    pub(crate) fn try_pop(&self) -> Result<T, QueueError> {
        let tail = self.tail.load(Ordering::Relaxed);

        if tail == self.head_cached.get() {
            // Cache miss — reload head from the producer.
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            if tail == head {
                self.instr.on_pop_empty();
                return Err(QueueError::Empty);
            }
        }

        let value = crate::raw::read_slot(&self.storage, tail);

        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.tail.store(next_tail, Ordering::Release);
        self.instr.on_pop();
        Ok(value)
    }

    /// Number of items currently in the ring.
    ///
    /// This is a snapshot and may be stale by the time it returns
    /// in a concurrent context.
    pub(crate) fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let cap = self.storage.capacity();
        (head + cap - tail) % cap
    }

    /// Whether the ring is currently empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Access the instrumentation.
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
        // Drop remaining items for T: Drop types.
        // No ordering needed — we're the sole owner during drop.
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        crate::raw::drop_range::<T, S, I>(&self.storage, tail, head);
    }
}
```

- [ ] **Step 4: Wire up module in `lib.rs`**

Add `mod engine;` to `crates/queue/src/lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: PASS — all engine tests pass

- [ ] **Step 6: Run clippy + miri**

Run:
```bash
cargo clippy -p mantis-queue --all-targets --all-features -- -D warnings
cargo +nightly miri test -p mantis-queue --all-features
```

- [ ] **Step 7: Commit**

```bash
git add crates/queue/src/engine.rs crates/queue/src/lib.rs
git commit -m "feat(queue): add RingEngine with Acquire/Release + cached indices"
```

---

## Task 7: Split handles (`Producer`/`Consumer`) and `RawRing`

**Files:**
- Create: `crates/queue/src/handle.rs`
- Modify: `crates/queue/src/lib.rs`

- [ ] **Step 1: Write failing tests**

In `crates/queue/src/handle.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    #[test]
    fn split_handles_push_pop() {
        let (mut tx, mut rx) = spsc_ring::<u64, 4>();
        assert!(tx.try_push(10).is_ok());
        assert_eq!(rx.try_pop().ok(), Some(10));
    }

    #[test]
    fn raw_ring_push_pop() {
        let mut ring = RawRing::<
            u64,
            InlineStorage<u64, 4>,
            Pow2Masked,
            ImmediatePush,
            NoInstr,
        >::new(InlineStorage::new(), NoInstr);
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
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p mantis-queue --all-features`
Expected: FAIL

- [ ] **Step 3: Implement handles**

In `crates/queue/src/handle.rs`:

```rust
//! Safe split handles for the SPSC ring buffer.
//!
//! `Producer` and `Consumer` each own one side of the ring and are
//! `Send` but not `Sync`. `RawRing` provides direct unsplit access
//! for single-threaded use (replay, benchmarking).

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::sync::Arc;

use core::marker::PhantomData;

use mantis_core::{ImmediatePush, IndexStrategy, Instrumentation,
    NoInstr, Pow2Masked, PushPolicy};
use mantis_types::{PushError, QueueError};

use crate::engine::RingEngine;
use crate::storage::{InlineStorage, Storage};

#[cfg(feature = "alloc")]
use crate::storage::HeapStorage;

/// Producer handle for the SPSC ring.
///
/// `Send` but `!Sync` — only one thread may push.
pub struct Producer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    #[cfg(feature = "alloc")]
    engine: Arc<RingEngine<T, S, I, P, Instr>>,
    #[cfg(not(feature = "alloc"))]
    engine: *const RingEngine<T, S, I, P, Instr>,
    _not_sync: PhantomData<*const ()>,
}

/// Consumer handle for the SPSC ring.
///
/// `Send` but `!Sync` — only one thread may pop.
pub struct Consumer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    #[cfg(feature = "alloc")]
    engine: Arc<RingEngine<T, S, I, P, Instr>>,
    #[cfg(not(feature = "alloc"))]
    engine: *const RingEngine<T, S, I, P, Instr>,
    _not_sync: PhantomData<*const ()>,
}

// SAFETY: Producer only accesses the producer side of the engine
// (head, tail_cached, slot writes). It is safe to send to another
// thread. Not Sync because only one thread may push.
#[allow(unsafe_code)]
unsafe impl<T, S, I, P, Instr> Send
    for Producer<T, S, I, P, Instr>
where
    T: Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{}

// SAFETY: Consumer only accesses the consumer side of the engine
// (tail, head_cached, slot reads). Safe to send to another thread.
#[allow(unsafe_code)]
unsafe impl<T, S, I, P, Instr> Send
    for Consumer<T, S, I, P, Instr>
where
    T: Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{}

#[cfg(feature = "alloc")]
impl<T, S, I, P, Instr> Producer<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    /// Try to push a value.
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
    /// Try to pop a value.
    #[inline]
    pub fn try_pop(&mut self) -> Result<T, QueueError> {
        self.engine.try_pop()
    }
}

/// Create an inline-storage SPSC ring, split into producer/consumer.
///
/// Requires the `alloc` feature for `Arc`-based shared ownership.
#[cfg(feature = "alloc")]
pub fn spsc_ring<T: Send, const N: usize>() -> (
    Producer<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr>,
    Consumer<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr>,
) {
    let engine = Arc::new(RingEngine::new(InlineStorage::new(), NoInstr));
    let tx = Producer {
        engine: Arc::clone(&engine),
        _not_sync: PhantomData,
    };
    let rx = Consumer {
        engine,
        _not_sync: PhantomData,
    };
    (tx, rx)
}

/// Create a heap-storage SPSC ring, split into producer/consumer.
///
/// Capacity is rounded up to the next power of two.
#[cfg(feature = "alloc")]
pub fn spsc_ring_heap<T: Send>(capacity: usize) -> (
    Producer<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr>,
    Consumer<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr>,
) {
    let engine = Arc::new(RingEngine::new(
        HeapStorage::new(capacity),
        NoInstr,
    ));
    let tx = Producer {
        engine: Arc::clone(&engine),
        _not_sync: PhantomData,
    };
    let rx = Consumer {
        engine,
        _not_sync: PhantomData,
    };
    (tx, rx)
}

/// Direct ring access without split handles.
///
/// For single-threaded replay, benchmarking, and power users.
/// No `Arc`, no handle overhead.
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
    /// Create a new raw ring with the given storage.
    pub fn new(storage: S, instr: Instr) -> Self {
        Self {
            engine: RingEngine::new(storage, instr),
        }
    }

    /// Try to push a value.
    #[inline]
    pub fn try_push(&mut self, value: T) -> Result<(), PushError<T>> {
        self.engine.try_push(value)
    }

    /// Try to pop a value.
    #[inline]
    pub fn try_pop(&mut self) -> Result<T, QueueError> {
        self.engine.try_pop()
    }

    /// Number of items in the ring.
    pub fn len(&self) -> usize {
        self.engine.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.engine.is_empty()
    }

    /// Usable capacity.
    pub fn capacity(&self) -> usize {
        self.engine.capacity()
    }

    /// Access the instrumentation.
    pub fn instrumentation(&self) -> &Instr {
        self.engine.instrumentation()
    }
}
```

- [ ] **Step 4: Wire up module and re-exports in `lib.rs`**

Update `crates/queue/src/lib.rs` to add:

```rust
mod handle;

pub use handle::{Consumer, Producer, RawRing};

#[cfg(feature = "alloc")]
pub use handle::{spsc_ring, spsc_ring_heap};
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: PASS

- [ ] **Step 6: Run clippy + miri**

```bash
cargo clippy -p mantis-queue --all-targets --all-features -- -D warnings
cargo +nightly miri test -p mantis-queue --all-features
```

- [ ] **Step 7: Commit**

```bash
git add crates/queue/src/handle.rs crates/queue/src/lib.rs
git commit -m "feat(queue): add Producer/Consumer split handles and RawRing"
```

**Deferred:** `try_push_batch`/`try_pop_batch` (spec Section 2.1) are deferred to a follow-up. They require `T: Copy` bounds and careful batch index management. The single-item API is the priority; batch ops will be added after benchmarks show they matter.

---

## Task 8: Preset type aliases and public API polish

**Files:**
- Create: `crates/queue/src/presets.rs`
- Modify: `crates/queue/src/lib.rs`

- [ ] **Step 1: Write failing tests**

In `crates/queue/src/presets.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mantis_core::CountingInstr;

    #[test]
    fn spsc_ring_preset_works() {
        let mut ring = SpscRing::<u64, 8>::new();
        assert!(ring.try_push(1).is_ok());
        assert_eq!(ring.try_pop().ok(), Some(1));
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn heap_preset_works() {
        let mut ring = SpscRingHeap::<u64>::with_capacity(8);
        assert!(ring.try_push(3).is_ok());
        assert_eq!(ring.try_pop().ok(), Some(3));
    }

    #[test]
    fn instrumented_preset_tracks() {
        let mut ring = SpscRingInstrumented::<u64, 8>::new();
        assert!(ring.try_push(1).is_ok());
        let _ = ring.try_pop();
        let _ = ring.try_pop(); // will be empty
        let instr = ring.instrumentation();
        assert_eq!(instr.push_count(), 1);
        assert_eq!(instr.pop_count(), 1);
        assert_eq!(instr.pop_empty_count(), 1);
    }
}
```

- [ ] **Step 2: Implement presets**

In `crates/queue/src/presets.rs`:

```rust
//! Curated preset type aliases for common SPSC ring configurations.

use mantis_core::{CountingInstr, ImmediatePush, NoInstr, Pow2Masked};

use crate::handle::RawRing;
use crate::storage::InlineStorage;

#[cfg(feature = "alloc")]
use crate::storage::HeapStorage;

/// Default SPSC ring — inline storage, cache-padded, no instrumentation.
///
/// This is the recommended preset for multi-threaded use. Cache padding
/// (128-byte alignment) is always applied inside `RingEngine` to prevent
/// false sharing between producer and consumer cache lines.
///
/// Also serves as the differential testing baseline.
pub type SpscRing<T, const N: usize> =
    RawRing<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr>;

impl<T, const N: usize> SpscRing<T, N> {
    /// Create a new SPSC ring.
    pub fn new() -> Self {
        RawRing::new(InlineStorage::new(), NoInstr)
    }
}

impl<T, const N: usize> Default for SpscRing<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: SpscRingPortable (without padding) and SpscRingPadded
// (with padding) are deferred. Currently CachePadded is always used
// inside RingEngine. Differentiating them requires a PaddingStrategy
// type parameter on the engine. This will be added after benchmarks
// show the padding overhead matters for small buffers.
// For now, SpscRing is the single inline preset (always padded).

/// Heap-allocated SPSC ring — runtime-sized.
#[cfg(feature = "alloc")]
pub type SpscRingHeap<T> =
    RawRing<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr>;

#[cfg(feature = "alloc")]
impl<T> SpscRingHeap<T> {
    /// Create a new heap ring with at least `capacity` slots.
    pub fn with_capacity(capacity: usize) -> Self {
        RawRing::new(HeapStorage::new(capacity), NoInstr)
    }
}

/// Instrumented SPSC ring — tracks push/pop/full/empty counts.
pub type SpscRingInstrumented<T, const N: usize> =
    RawRing<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, CountingInstr>;

impl<T, const N: usize> SpscRingInstrumented<T, N> {
    /// Create a new instrumented ring.
    pub fn new() -> Self {
        RawRing::new(InlineStorage::new(), CountingInstr::new())
    }
}

impl<T, const N: usize> Default for SpscRingInstrumented<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Wire up in `lib.rs`**

Add to `crates/queue/src/lib.rs`:

```rust
mod presets;

pub use presets::*;
```

Also ensure all public re-exports are clean:

```rust
pub use mantis_core::{CountingInstr, ImmediatePush, NoInstr, Pow2Masked};
pub use mantis_types::{PushError, QueueError};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: PASS — all preset tests pass

- [ ] **Step 5: Run full workspace check**

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

- [ ] **Step 6: Commit**

```bash
git add crates/queue/src/presets.rs crates/queue/src/lib.rs
git commit -m "feat(queue): add SpscRing preset type aliases"
```

---

## Task 9: Integration tests — all presets + Drop safety

**Files:**
- Create: `crates/queue/tests/spsc_basic.rs`

- [ ] **Step 1: Write integration tests**

```rust
//! Integration tests for all SPSC ring presets.

use mantis_queue::{
    QueueError, SpscRing, SpscRingInstrumented,
};

#[cfg(feature = "alloc")]
use mantis_queue::{spsc_ring, spsc_ring_heap, SpscRingHeap};

#[test]
fn spsc_ring_fill_and_drain() {
    let mut ring = SpscRing::<u64, 8>::new();
    for i in 0..7 {
        assert!(ring.try_push(i).is_ok());
    }
    assert!(ring.try_push(99).is_err());
    for i in 0..7 {
        assert_eq!(ring.try_pop().ok(), Some(i));
    }
    assert_eq!(ring.try_pop(), Err(QueueError::Empty));
}

#[test]
fn spsc_ring_fill_and_drain_capacity_16() {
    let mut ring = SpscRing::<u64, 16>::new();
    for i in 0..15 {
        assert!(ring.try_push(i).is_ok());
    }
    for i in 0..15 {
        assert_eq!(ring.try_pop().ok(), Some(i));
    }
}

#[cfg(feature = "alloc")]
#[test]
fn heap_fill_and_drain() {
    let mut ring = SpscRingHeap::<u64>::with_capacity(8);
    for i in 0..7 {
        assert!(ring.try_push(i).is_ok());
    }
    for i in 0..7 {
        assert_eq!(ring.try_pop().ok(), Some(i));
    }
}

#[test]
fn instrumented_tracks_all_events() {
    let mut ring = SpscRingInstrumented::<u64, 4>::new();
    assert!(ring.try_push(1).is_ok());
    assert!(ring.try_push(2).is_ok());
    assert!(ring.try_push(3).is_ok());
    assert!(ring.try_push(4).is_err()); // full
    let _ = ring.try_pop();
    let _ = ring.try_pop();
    let _ = ring.try_pop();
    let _ = ring.try_pop(); // empty

    let instr = ring.instrumentation();
    assert_eq!(instr.push_count(), 3);
    assert_eq!(instr.pop_count(), 3);
    assert_eq!(instr.push_full_count(), 1);
    assert_eq!(instr.pop_empty_count(), 1);
}

/// Verify that T: Drop types are correctly dropped when the ring
/// is dropped with un-popped elements.
#[cfg(feature = "alloc")]
#[test]
fn drop_safety_with_string() {
    extern crate alloc;
    use alloc::string::String;

    let mut ring = SpscRing::<String, 4>::new();
    ring.try_push(String::from("alpha")).ok();
    ring.try_push(String::from("beta")).ok();
    // Pop one, leave one in the ring.
    let _ = ring.try_pop();
    // Dropping ring should not leak "beta".
    drop(ring);
    // If we get here without asan/miri complaints, drop is correct.
}

/// Verify that wraparound works correctly with many rounds.
#[test]
fn extensive_wraparound() {
    let mut ring = SpscRing::<u64, 4>::new();
    for round in 0..100 {
        for i in 0..3 {
            let val = round * 3 + i;
            assert!(ring.try_push(val).is_ok(), "push failed at {val}");
        }
        for i in 0..3 {
            let expected = round * 3 + i;
            assert_eq!(ring.try_pop().ok(), Some(expected));
        }
    }
}

#[cfg(feature = "alloc")]
#[test]
fn split_handles_two_thread() {
    use std::thread;

    let (mut tx, mut rx) = spsc_ring::<u64, 1024>();
    let count = 10_000u64;

    let producer = thread::spawn(move || {
        for i in 0..count {
            while tx.try_push(i).is_err() {
                core::hint::spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        for i in 0..count {
            loop {
                match rx.try_pop() {
                    Ok(val) => {
                        assert_eq!(val, i, "FIFO violation");
                        break;
                    }
                    Err(_) => core::hint::spin_loop(),
                }
            }
        }
    });

    producer.join().expect("producer panicked");
    consumer.join().expect("consumer panicked");
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: PASS — all integration tests pass

- [ ] **Step 3: Run miri on the full test suite**

Run: `cargo +nightly miri test -p mantis-queue --all-features`
Expected: PASS — no undefined behavior detected

Note: The two-thread test may need `MIRIFLAGS="-Zmiri-ignore-leaks"` or reduced iteration count under miri.

- [ ] **Step 4: Commit**

```bash
git add crates/queue/tests/spsc_basic.rs
git commit -m "test(queue): add integration tests for all SPSC presets"
```

---

## Task 10: Stress test — two-thread 10M items

**Files:**
- Create: `crates/queue/tests/spsc_stress.rs`

- [ ] **Step 1: Write stress test**

```rust
//! Two-thread stress test: 10M sequential u64 values.
//!
//! Verifies FIFO ordering and data integrity under sustained load.
//! Run under miri with reduced count for data-race detection.

#[cfg(feature = "alloc")]
#[test]
fn stress_10m_items() {
    use std::thread;

    use mantis_queue::spsc_ring;

    let item_count: u64 = if cfg!(miri) { 1_000 } else { 10_000_000 };

    let (mut tx, mut rx) = spsc_ring::<u64, 4096>();

    let producer = thread::spawn(move || {
        for i in 0..item_count {
            while tx.try_push(i).is_err() {
                core::hint::spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut expected = 0u64;
        while expected < item_count {
            match rx.try_pop() {
                Ok(val) => {
                    assert_eq!(
                        val, expected,
                        "FIFO violation: expected {expected}, got {val}"
                    );
                    expected += 1;
                }
                Err(_) => core::hint::spin_loop(),
            }
        }
    });

    producer.join().expect("producer panicked");
    consumer.join().expect("consumer panicked");
}
```

- [ ] **Step 2: Run the stress test**

Run: `cargo test -p mantis-queue --all-features -- stress_10m --nocapture`
Expected: PASS (takes a few seconds)

- [ ] **Step 3: Run under miri (reduced count)**

Run: `cargo +nightly miri test -p mantis-queue --all-features -- stress_10m`
Expected: PASS with 1000 items under miri

- [ ] **Step 4: Commit**

```bash
git add crates/queue/tests/spsc_stress.rs
git commit -m "test(queue): add 10M-item two-thread stress test"
```

---

## Task 11: Update `docs/PROGRESS.md`

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: Update Phase 1 checkboxes**

Mark completed items in Section 1.1 (SPSC Ring Buffer):
- [x] Core ring buffer engine with strategy pattern
- [x] `raw` submodule with unsafe slot operations
- [x] Power-of-2 masked index implementation
- [x] Cache-padded variant to prevent false sharing
- [x] Portable baseline implementation
- [x] Preset type aliases (`SpscRing`, `SpscRingHeap`, `SpscRingInstrumented`)
- [x] Unit tests (push/pop, full/empty, wraparound)
- [x] Miri validation

Update the crate status table:
| `mantis-core` | Active | yes | 1 | — | — |
| `mantis-types` | Active | yes | 6 | — | — |
| `mantis-queue` | Active | yes | ~20 | — | — |

- [ ] **Step 2: Commit**

```bash
git add docs/PROGRESS.md
git commit -m "docs: update PROGRESS.md with SPSC ring completion"
```

---

## Summary

| Task | What | Commit |
|---|---|---|
| 1 | `PushError<T>` in mantis-types | `feat(types): add PushError<T>` |
| 2 | `CountingInstr` in mantis-core | `feat(core): add CountingInstr` |
| 3 | `CachePadded<T>` in mantis-queue | `feat(queue): add CachePadded<T>` |
| 4 | `Storage` trait + Inline/Heap | `feat(queue): add Storage trait` |
| 5 | `raw::slot` unsafe ops | `feat(queue): add raw::slot` |
| 6 | `RingEngine` core | `feat(queue): add RingEngine` |
| 7 | `Producer`/`Consumer`/`RawRing` | `feat(queue): add handles` |
| 8 | Preset type aliases | `feat(queue): add presets` |
| 9 | Integration tests | `test(queue): integration tests` |
| 10 | Stress test 10M items | `test(queue): stress test` |
| 11 | Progress doc update | `docs: update PROGRESS.md` |

**Total: 11 tasks, ~11 commits, estimated ~50 test cases.**

After this plan: proceed to Plan 2 (Benchmark Harness), Plan 3 (Verification), Plan 4 (CI Improvements).
