# SPSC Copy-Optimized Ring Buffer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `T: Copy` SPSC ring with SIMD copy kernels, batch push/pop, and cold-path hints alongside the existing ring, benchmarked side-by-side.

**Architecture:** New `copy_ring` module in `mantis-queue` with its own engine, raw submodule, and presets. Shares strategy traits from `mantis-core` (plus new `CopyPolicy`). Platform SIMD kernels (SSE2/NEON) selected at compile time. Nightly feature gates `likely`/`unlikely`/`cold_path`/`generic_const_exprs`.

**Tech Stack:** Rust stable + optional nightly, `core::arch::x86_64` (SSE2), `core::arch::aarch64` (NEON), Criterion benchmarks.

**Spec:** `docs/specs/2026-03-25-spsc-copy-ring-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|---|---|
| `crates/queue/src/copy_ring/mod.rs` | Module root, `RawRingCopy` public handle |
| `crates/queue/src/copy_ring/engine.rs` | `CopyRingEngine` — push, pop, push_batch, pop_batch |
| `crates/queue/src/copy_ring/raw/mod.rs` | Safe wrappers, `unsafe impl Sync`, `DefaultCopyPolicy` |
| `crates/queue/src/copy_ring/raw/simd.rs` | `CopyDispatcher`, platform intrinsics, exact-size kernels |
| `crates/queue/src/copy_ring/handle.rs` | `ProducerCopy`, `ConsumerCopy` split handles (behind `alloc`) |
| `crates/bench/src/messages.rs` | `Message48`, `Message64`, `make_msg48`, `make_msg64` |
| `crates/queue/tests/spsc_copy_basic.rs` | Integration + stress tests for copy ring |

### Modified Files

| File | Change |
|---|---|
| `crates/core/src/lib.rs` | Add `CopyPolicy` trait |
| `crates/queue/src/lib.rs` | Add `mod copy_ring`, re-export new presets |
| `crates/queue/src/presets.rs` | Add `SpscRingCopy`, `SpscRingCopyHeap`, `SpscRingCopyInstrumented` |
| `crates/queue/src/engine.rs` | Add `#[cold]` slow paths, nightly `cold_path` |
| `crates/queue/Cargo.toml` | Add `nightly` feature |
| `crates/bench/src/lib.rs` | Add `pub mod messages` |
| `crates/bench/benches/spsc_mantis.rs` | Add copy-ring + Message benchmarks |
| `crates/bench/src/workloads.rs` | Add batch workload functions |
| `crates/queue/examples/asm_shim.rs` | Add copy-ring shims |

---

## Task 1: CopyPolicy Trait + Nightly Feature Flag

**Files:**
- Modify: `crates/core/src/lib.rs`
- Modify: `crates/queue/Cargo.toml`
- Modify: `crates/queue/src/lib.rs`

- [ ] **Step 1: Add `CopyPolicy` trait to mantis-core**

Add after the `Instrumentation` trait in `crates/core/src/lib.rs`:

```rust
/// Copy strategy for SPSC ring slot operations.
///
/// Implementations are zero-sized types used for static dispatch only.
/// No instance is ever constructed — all methods are associated functions.
pub trait CopyPolicy<T: Copy> {
    /// Copy `*src` into the ring slot at `*dst`.
    ///
    /// # Safety
    /// - `dst` must be valid, aligned, and point to an unoccupied slot.
    /// - `src` must be valid and aligned for reads of `T`.
    unsafe fn copy_in(dst: *mut T, src: *const T);

    /// Copy the ring slot at `*src` into `*dst`.
    ///
    /// # Safety
    /// - `src` must be valid, aligned, and point to an occupied slot.
    /// - `dst` must be valid and aligned for writes of `T`.
    unsafe fn copy_out(dst: *mut T, src: *const T);
}
```

- [ ] **Step 2: Add `nightly` feature to mantis-queue**

In `crates/queue/Cargo.toml`, add to `[features]`:

```toml
nightly = []
```

- [ ] **Step 3: Add nightly cfg_attr to queue crate root**

At the top of `crates/queue/src/lib.rs`, add before any other attributes:

```rust
#![cfg_attr(
    feature = "nightly",
    feature(likely_unlikely, generic_const_exprs)
)]
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p mantis-core -p mantis-queue --all-features`
Expected: success, no warnings

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/lib.rs crates/queue/Cargo.toml crates/queue/src/lib.rs
git commit -m "feat(core): add CopyPolicy trait and nightly feature flag"
```

---

## Task 2: SIMD Copy Kernels + DefaultCopyPolicy

**Files:**
- Create: `crates/queue/src/copy_ring/raw/simd.rs`
- Create: `crates/queue/src/copy_ring/raw/mod.rs`
- Create: `crates/queue/src/copy_ring/mod.rs`
- Modify: `crates/queue/src/lib.rs`

- [ ] **Step 1: Write SIMD kernel tests**

At the bottom of `crates/queue/src/copy_ring/raw/simd.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_u64_scalar() {
        let src: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let mut dst: u64 = 0;
        unsafe {
            CopyDispatcher::<u64, { core::mem::size_of::<u64>() }>::copy(
                &mut dst as *mut u64,
                &src as *const u64,
            );
        }
        assert_eq!(dst, src);
    }

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg16 {
        a: u64,
        b: u64,
    }
    const _: () = assert!(core::mem::size_of::<Msg16>() == 16);

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg32 {
        a: u64, b: u64, c: u64, d: u64,
    }
    const _: () = assert!(core::mem::size_of::<Msg32>() == 32);

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg48 {
        a: u64, b: u64, c: u64, d: u64, e: u64, f: u64,
    }
    const _: () = assert!(core::mem::size_of::<Msg48>() == 48);

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg64 {
        a: u64, b: u64, c: u64, d: u64,
        e: u64, f: u64, g: u64, h: u64,
    }
    const _: () = assert!(core::mem::size_of::<Msg64>() == 64);

    macro_rules! test_copy_roundtrip {
        ($name:ident, $ty:ty, $val:expr) => {
            #[test]
            fn $name() {
                let src: $ty = $val;
                let mut dst = <$ty>::default();
                unsafe {
                    CopyDispatcher::<$ty, { core::mem::size_of::<$ty>() }>::copy(
                        &mut dst as *mut $ty,
                        &src as *const $ty,
                    );
                }
                assert_eq!(dst, src);
            }
        };
    }

    test_copy_roundtrip!(copy_16_bytes, Msg16, Msg16 { a: 1, b: 2 });
    test_copy_roundtrip!(copy_32_bytes, Msg32, Msg32 { a: 1, b: 2, c: 3, d: 4 });
    test_copy_roundtrip!(copy_48_bytes, Msg48, Msg48 { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6 });
    test_copy_roundtrip!(copy_64_bytes, Msg64, Msg64 {
        a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8,
    });

    // Test bucket fallback (24 bytes = copy_16 + 8 scalar)
    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg24 {
        a: u64, b: u64, c: u64,
    }
    const _: () = assert!(core::mem::size_of::<Msg24>() == 24);

    test_copy_roundtrip!(copy_24_bytes_bucket, Msg24, Msg24 { a: 1, b: 2, c: 3 });

    // Test large type (> 64 bytes, falls back to ptr::copy_nonoverlapping)
    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg128 {
        data: [u64; 16],
    }
    const _: () = assert!(core::mem::size_of::<Msg128>() == 128);

    #[test]
    fn copy_128_bytes_fallback() {
        let src = Msg128 { data: [42; 16] };
        let mut dst = Msg128::default();
        unsafe {
            CopyDispatcher::<Msg128, { core::mem::size_of::<Msg128>() }>::copy(
                &mut dst as *mut Msg128,
                &src as *const Msg128,
            );
        }
        assert_eq!(dst, src);
    }
}
```

- [ ] **Step 2: Implement SIMD kernels**

Create `crates/queue/src/copy_ring/raw/simd.rs`:

```rust
//! SIMD copy dispatch — platform-specific kernels for exact message sizes.
//!
//! All unsafe code in this module is for SIMD intrinsics and raw pointer copies.
//! The `CopyDispatcher` selects the optimal kernel at compile time based on
//! `size_of::<T>()`.
#![allow(unsafe_code)]

use core::marker::PhantomData;
use core::ptr;

use mantis_core::CopyPolicy;

// ─── Platform intrinsics ────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_storeu_si128};

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::{uint8x16_t, vld1q_u8, vst1q_u8};

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn load128(src: *const u8) -> __m128i {
    _mm_loadu_si128(src.cast::<__m128i>())
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn store128(dst: *mut u8, v: __m128i) {
    _mm_storeu_si128(dst.cast::<__m128i>(), v);
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn load128(src: *const u8) -> uint8x16_t {
    vld1q_u8(src)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn store128(dst: *mut u8, v: uint8x16_t) {
    vst1q_u8(dst, v);
}

// ─── Exact-size kernels (macro-generated) ───────────────────────────────

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
macro_rules! define_copy_exact {
    ($name:ident, $bytes:expr, $chunks:expr) => {
        #[inline(always)]
        unsafe fn $name(dst: *mut u8, src: *const u8) {
            let _ = $bytes; // documents intended size
            let mut i = 0usize;
            while i < $chunks {
                let v = load128(src.add(i * 16));
                store128(dst.add(i * 16), v);
                i += 1;
            }
        }
    };
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
define_copy_exact!(copy_16, 16, 1);
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
define_copy_exact!(copy_32, 32, 2);
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
define_copy_exact!(copy_48, 48, 3);
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
define_copy_exact!(copy_64, 64, 4);

// ─── Bucket fallback ────────────────────────────────────────────────────

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[inline(always)]
unsafe fn copy_bucket<const BASE: usize, const N: usize>(
    dst: *mut u8,
    src: *const u8,
) {
    match BASE {
        16 => copy_16(dst, src),
        32 => copy_32(dst, src),
        48 => copy_48(dst, src),
        _ => {}
    }
    ptr::copy_nonoverlapping(src.add(BASE), dst.add(BASE), N - BASE);
}

// ─── Compile-time size dispatcher ───────────────────────────────────────

/// Dispatches to the optimal copy kernel based on the const size `N`.
///
/// Since `N` is a const generic, all branches are evaluated at compile time
/// and dead code is eliminated. For `Message48`, only `copy_48` remains.
pub struct CopyDispatcher<T, const N: usize>(PhantomData<T>);

impl<T: Copy, const N: usize> CopyDispatcher<T, N> {
    #[inline(always)]
    pub unsafe fn copy(dst: *mut T, src: *const T) {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            if N <= 8 {
                ptr::copy_nonoverlapping(src, dst, 1);
                return;
            }
            if N == 16 { return copy_16(dst.cast(), src.cast()); }
            if N == 32 { return copy_32(dst.cast(), src.cast()); }
            if N == 48 { return copy_48(dst.cast(), src.cast()); }
            if N == 64 { return copy_64(dst.cast(), src.cast()); }
            if N < 32 { return copy_bucket::<16, N>(dst.cast(), src.cast()); }
            if N < 48 { return copy_bucket::<32, N>(dst.cast(), src.cast()); }
            if N < 64 { return copy_bucket::<48, N>(dst.cast(), src.cast()); }
        }
        // Scalar fallback for unsupported arch or N > 64
        ptr::copy_nonoverlapping(src, dst, 1);
    }
}

// ─── DefaultCopyPolicy ─────────────────────────────────────────────────

/// Default copy policy using compile-time SIMD dispatch.
pub struct DefaultCopyPolicy;

impl<T: Copy> CopyPolicy<T> for DefaultCopyPolicy {
    #[inline(always)]
    unsafe fn copy_in(dst: *mut T, src: *const T) {
        CopyDispatcher::<T, { core::mem::size_of::<T>() }>::copy(dst, src);
    }

    #[inline(always)]
    unsafe fn copy_out(dst: *mut T, src: *const T) {
        CopyDispatcher::<T, { core::mem::size_of::<T>() }>::copy(dst, src);
    }
}

// Tests at bottom of file (see Step 1)
```

- [ ] **Step 3: Create raw/mod.rs with safe wrappers**

Create `crates/queue/src/copy_ring/raw/mod.rs`:

```rust
//! Unsafe internals for the copy-optimized ring.
//!
//! All unsafe code for the copy ring lives in this module and its submodules.
#![allow(unsafe_code)]

pub(crate) mod simd;

pub(crate) use simd::DefaultCopyPolicy;

use mantis_core::CopyPolicy;

use crate::storage::Storage;

/// Write `value` into the slot at `index` using the given copy policy.
///
/// # Safety
/// Caller must ensure:
/// - `index < storage.capacity()`
/// - The slot at `index` is logically unoccupied (producer has exclusive access)
/// - No other thread is reading this slot concurrently
#[inline(always)]
pub(crate) unsafe fn write_slot_copy<T: Copy, S: Storage<T>, CP: CopyPolicy<T>>(
    storage: &S,
    index: usize,
    value: &T,
) {
    // SAFETY: index < capacity guaranteed by caller. Slot is logically
    // unoccupied and being initialized — producer exclusively owns slots
    // from tail_cached..head (SPSC protocol). MaybeUninit<T> is being
    // written to via cast to *mut T, which is valid because T: Copy has
    // no drop glue and the pointer is properly aligned by Storage.
    let dst = storage.slot_ptr(index).cast::<T>();
    CP::copy_in(dst, value as *const T);
}

/// Read value from the slot at `index` using the given copy policy.
///
/// # Safety
/// Caller must ensure:
/// - `index < storage.capacity()`
/// - The slot at `index` is logically occupied (consumer has exclusive access)
/// - No other thread is writing this slot concurrently
#[inline(always)]
pub(crate) unsafe fn read_slot_copy<T: Copy, S: Storage<T>, CP: CopyPolicy<T>>(
    storage: &S,
    index: usize,
    out: &mut T,
) {
    // SAFETY: index < capacity guaranteed by caller. Slot is logically
    // occupied and initialized — consumer exclusively owns slots from
    // tail..head_cached (SPSC protocol). The slot was previously written
    // by the producer, so the MaybeUninit<T> is initialized. Reading via
    // cast to *const T is valid because T: Copy and the pointer is aligned.
    let src = storage.slot_ptr(index).cast::<T>();
    CP::copy_out(out as *mut T, src);
}
```

- [ ] **Step 4: Create copy_ring/mod.rs stub**

Create `crates/queue/src/copy_ring/mod.rs`:

```rust
//! Copy-optimized SPSC ring buffer for `T: Copy` types.
//!
//! Provides SIMD-accelerated slot copies, batch push/pop, and
//! cold-path hints for sub-nanosecond hot paths.

pub(crate) mod engine;
#[cfg(feature = "alloc")]
pub(crate) mod handle;
pub(crate) mod raw;
```

- [ ] **Step 5: Create engine.rs stub so it compiles**

Create `crates/queue/src/copy_ring/engine.rs`:

```rust
//! Copy-optimized ring engine (placeholder — implemented in Task 3).
```

- [ ] **Step 6: Create handle.rs stub so it compiles**

Create `crates/queue/src/copy_ring/handle.rs`:

```rust
//! Split producer/consumer handles for copy ring (placeholder — implemented in Task 5).
```

- [ ] **Step 7: Wire up the module in lib.rs**

In `crates/queue/src/lib.rs`, add after the existing `mod raw;` line:

```rust
mod copy_ring;
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: all existing tests pass + new SIMD tests pass

- [ ] **Step 9: Run Miri (verifies scalar fallback path is sound)**

Run: `cargo +nightly miri test -p mantis-queue --all-features`
Expected: no UB detected

- [ ] **Step 10: Commit**

```bash
git add crates/queue/src/copy_ring/
git commit -m "feat(queue): add SIMD copy kernels with compile-time size dispatch"
```

---

## Task 3: CopyRingEngine — Single Push/Pop

**Files:**
- Modify: `crates/queue/src/copy_ring/engine.rs`
- Modify: `crates/queue/src/copy_ring/raw/mod.rs`

- [ ] **Step 1: Write engine unit tests for single push/pop**

Add to the bottom of `crates/queue/src/copy_ring/engine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InlineStorage;
    use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

    type TestEngine = CopyRingEngine<
        u64,
        InlineStorage<u64, 8>,
        Pow2Masked,
        ImmediatePush,
        NoInstr,
        crate::copy_ring::raw::DefaultCopyPolicy,
    >;

    fn new_engine() -> TestEngine {
        CopyRingEngine::new(InlineStorage::new(), NoInstr)
    }

    #[test]
    fn push_pop_single() {
        let engine = new_engine();
        let val = 42u64;
        let mut out = 0u64;
        assert!(engine.push(&val));
        assert!(engine.pop(&mut out));
        assert_eq!(out, 42);
    }

    #[test]
    fn push_full_returns_false() {
        let engine = new_engine();
        // capacity = 8, usable = 7 (sentinel slot)
        for i in 0u64..7 {
            assert!(engine.push(&i), "push {i} should succeed");
        }
        assert!(!engine.push(&99), "push to full ring should return false");
    }

    #[test]
    fn pop_empty_returns_false() {
        let engine = new_engine();
        let mut out = 0u64;
        assert!(!engine.pop(&mut out), "pop from empty ring should return false");
    }

    #[test]
    fn fifo_ordering() {
        let engine = new_engine();
        for i in 0u64..7 {
            assert!(engine.push(&i));
        }
        for i in 0u64..7 {
            let mut out = 0u64;
            assert!(engine.pop(&mut out));
            assert_eq!(out, i);
        }
    }

    #[test]
    fn wraparound() {
        let engine = new_engine();
        // Push 5, pop 5, push 5 again — exercises index wrapping
        for round in 0..3 {
            for i in 0u64..5 {
                assert!(engine.push(&(round * 10 + i)));
            }
            for i in 0u64..5 {
                let mut out = 0u64;
                assert!(engine.pop(&mut out));
                assert_eq!(out, round * 10 + i);
            }
        }
    }

    #[test]
    fn len_and_capacity() {
        let engine = new_engine();
        assert_eq!(engine.capacity(), 7); // 8 - 1 sentinel
        assert_eq!(engine.len(), 0);
        assert!(engine.is_empty());

        engine.push(&1);
        assert_eq!(engine.len(), 1);
        assert!(!engine.is_empty());

        let mut out = 0u64;
        engine.pop(&mut out);
        assert_eq!(engine.len(), 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mantis-queue --all-features`
Expected: FAIL — `CopyRingEngine` not found

- [ ] **Step 3: Implement CopyRingEngine**

Replace the contents of `crates/queue/src/copy_ring/engine.rs`:

```rust
//! Copy-optimized ring engine for `T: Copy` types.
//!
//! Same SPSC protocol as `RingEngine` but uses `CopyPolicy` for slot
//! operations and returns `bool` instead of `Result` (caller retains
//! the value since `T: Copy`).

use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};

use mantis_core::{CopyPolicy, IndexStrategy, Instrumentation, PushPolicy};

use crate::pad::CachePadded;
use crate::storage::Storage;

/// Cold slow-path for full ring. `#[cold]` tells LLVM to move this
/// out of the hot path even on stable.
#[cold]
#[inline(never)]
fn slow_full() -> bool {
    false
}

/// Cold slow-path for empty ring.
#[cold]
#[inline(never)]
fn slow_empty() -> bool {
    false
}

pub(crate) struct CopyRingEngine<T: Copy, S, I, P, Instr, CP> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    tail_cached: CachePadded<Cell<usize>>,
    head_cached: CachePadded<Cell<usize>>,
    storage: S,
    instr: Instr,
    _marker: PhantomData<(T, I, P, CP)>,
}

impl<T, S, I, P, Instr, CP> CopyRingEngine<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
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

    #[inline(always)]
    pub(crate) fn push(&self, value: &T) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = I::wrap(head + 1, self.storage.capacity());

        if next_head == self.tail_cached.get() {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            if next_head == tail {
                #[cfg(feature = "nightly")]
                core::hint::cold_path();
                self.instr.on_push_full();
                return slow_full();
            }
        }

        // SAFETY: head < capacity (bounded index from I::wrap).
        // Producer exclusively owns this slot (SPSC protocol).
        unsafe {
            crate::copy_ring::raw::write_slot_copy::<T, S, CP>(
                &self.storage,
                head,
                value,
            );
        }
        self.head.store(next_head, Ordering::Release);
        self.instr.on_push();
        true
    }

    #[inline(always)]
    pub(crate) fn pop(&self, out: &mut T) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);

        if tail == self.head_cached.get() {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            if tail == head {
                #[cfg(feature = "nightly")]
                core::hint::cold_path();
                self.instr.on_pop_empty();
                return slow_empty();
            }
        }

        // SAFETY: tail < capacity (bounded index from I::wrap).
        // Consumer exclusively owns this slot (SPSC protocol).
        unsafe {
            crate::copy_ring::raw::read_slot_copy::<T, S, CP>(
                &self.storage,
                tail,
                out,
            );
        }
        let next_tail = I::wrap(tail + 1, self.storage.capacity());
        self.tail.store(next_tail, Ordering::Release);
        self.instr.on_pop();
        true
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity() - 1
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let cap = self.storage.capacity();
        if head >= tail { head - tail } else { cap - tail + head }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn instrumentation(&self) -> &Instr {
        &self.instr
    }

    #[inline]
    pub(crate) fn storage(&self) -> &S {
        &self.storage
    }
}
```

- [ ] **Step 4: Add `unsafe impl Sync` to raw/mod.rs**

Append to `crates/queue/src/copy_ring/raw/mod.rs`:

```rust
use crate::copy_ring::engine::CopyRingEngine;

// SAFETY: SPSC protocol guarantees disjoint access:
// - Producer ONLY accesses: head (AtomicUsize), tail_cached (Cell<usize>)
// - Consumer ONLY accesses: tail (AtomicUsize), head_cached (Cell<usize>)
// The split-handle design enforces this partition at compile time.
// Validated by Miri on every PR.
unsafe impl<T, S, I, P, Instr, CP> Sync
    for CopyRingEngine<T, S, I, P, Instr, CP>
where
    T: Copy + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: all pass including new engine tests

- [ ] **Step 6: Run Miri**

Run: `cargo +nightly miri test -p mantis-queue --all-features`
Expected: no UB

- [ ] **Step 7: Commit**

```bash
git add crates/queue/src/copy_ring/engine.rs crates/queue/src/copy_ring/raw/mod.rs
git commit -m "feat(queue): add CopyRingEngine with single push/pop"
```

---

## Task 4: Batch Push/Pop

**Files:**
- Modify: `crates/queue/src/copy_ring/engine.rs`

- [ ] **Step 1: Write batch tests**

Add to the `tests` module in `crates/queue/src/copy_ring/engine.rs`:

```rust
    #[test]
    fn push_batch_full_capacity() {
        let engine = new_engine();
        let src: Vec<u64> = (0..7).collect();
        let pushed = engine.push_batch(&src);
        assert_eq!(pushed, 7);

        // Ring is now full, batch should push 0
        let more = [100u64, 101];
        assert_eq!(engine.push_batch(&more), 0);
    }

    #[test]
    fn pop_batch_all() {
        let engine = new_engine();
        let src: Vec<u64> = (0..5).collect();
        engine.push_batch(&src);

        let mut dst = vec![0u64; 5];
        let popped = engine.pop_batch(&mut dst);
        assert_eq!(popped, 5);
        assert_eq!(dst, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn push_batch_partial() {
        let engine = new_engine();
        // Fill 5 of 7 usable slots
        let first: Vec<u64> = (0..5).collect();
        assert_eq!(engine.push_batch(&first), 5);

        // Try to push 5 more — only 2 should fit
        let second: Vec<u64> = (10..15).collect();
        assert_eq!(engine.push_batch(&second), 2);
    }

    #[test]
    fn pop_batch_partial() {
        let engine = new_engine();
        let src: Vec<u64> = (0..3).collect();
        engine.push_batch(&src);

        // Request more than available
        let mut dst = vec![0u64; 10];
        let popped = engine.pop_batch(&mut dst);
        assert_eq!(popped, 3);
        assert_eq!(&dst[..3], &[0, 1, 2]);
    }

    #[test]
    fn batch_empty_slice() {
        let engine = new_engine();
        assert_eq!(engine.push_batch(&[]), 0);
        let mut dst = vec![0u64; 0];
        assert_eq!(engine.pop_batch(&mut dst), 0);
    }

    #[test]
    fn batch_wraparound() {
        let engine = new_engine();
        // Fill and drain to advance indices past the start
        let vals: Vec<u64> = (0..6).collect();
        engine.push_batch(&vals);
        let mut drain = vec![0u64; 6];
        engine.pop_batch(&mut drain);

        // Now head and tail are at index 6. Push 5 items — wraps around.
        let wrap: Vec<u64> = (100..105).collect();
        assert_eq!(engine.push_batch(&wrap), 5);
        let mut out = vec![0u64; 5];
        assert_eq!(engine.pop_batch(&mut out), 5);
        assert_eq!(out, vec![100, 101, 102, 103, 104]);
    }

    #[test]
    fn batch_fifo_ordering() {
        let engine = new_engine();
        for round in 0u64..10 {
            let src: Vec<u64> = (round * 7..(round + 1) * 7).collect();
            let pushed = engine.push_batch(&src);
            assert_eq!(pushed, src.len());

            let mut dst = vec![0u64; pushed];
            let popped = engine.pop_batch(&mut dst);
            assert_eq!(popped, pushed);
            assert_eq!(dst, src);
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mantis-queue --all-features`
Expected: FAIL — `push_batch` not found

- [ ] **Step 3: Implement push_batch and pop_batch**

Add to the `impl` block in `crates/queue/src/copy_ring/engine.rs`, after `pop`:

```rust
    #[inline(always)]
    pub(crate) fn push_batch(&self, src: &[T]) -> usize {
        if src.is_empty() {
            return 0;
        }

        let head = self.head.load(Ordering::Relaxed);
        let cached_tail = self.tail_cached.get();
        let cap = self.storage.capacity();
        let usable = cap - 1;

        let len = if head >= cached_tail {
            head - cached_tail
        } else {
            cap - cached_tail + head
        };
        let mut free = usable - len;

        if free == 0 {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            let len = if head >= tail {
                head - tail
            } else {
                cap - tail + head
            };
            free = usable - len;
            if free == 0 {
                return 0;
            }
        }

        let n = src.len().min(free);
        let mut idx = head;
        for item in &src[..n] {
            // SAFETY: idx < capacity (bounded by I::wrap).
            // Producer exclusively owns this slot.
            unsafe {
                crate::copy_ring::raw::write_slot_copy::<T, S, CP>(
                    &self.storage,
                    idx,
                    item,
                );
            }
            idx = I::wrap(idx + 1, cap);
        }

        // NOTE: Batch ops intentionally do NOT fire per-item instrumentation.
        // Instrumentation is advisory and adding N atomic increments in the
        // batch loop would defeat the purpose. Counters reflect single push/pop only.
        self.head.store(idx, Ordering::Release);
        n
    }

    #[inline(always)]
    pub(crate) fn pop_batch(&self, dst: &mut [T]) -> usize {
        if dst.is_empty() {
            return 0;
        }

        let tail = self.tail.load(Ordering::Relaxed);
        let cached_head = self.head_cached.get();
        let cap = self.storage.capacity();

        let mut avail = if cached_head >= tail {
            cached_head - tail
        } else {
            cap - tail + cached_head
        };

        if avail == 0 {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            avail = if head >= tail {
                head - tail
            } else {
                cap - tail + head
            };
            if avail == 0 {
                return 0;
            }
        }

        let n = dst.len().min(avail);
        let mut idx = tail;
        for out in &mut dst[..n] {
            // SAFETY: idx < capacity (bounded by I::wrap).
            // Consumer exclusively owns this slot.
            unsafe {
                crate::copy_ring::raw::read_slot_copy::<T, S, CP>(
                    &self.storage,
                    idx,
                    out,
                );
            }
            idx = I::wrap(idx + 1, cap);
        }

        self.tail.store(idx, Ordering::Release);
        n
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: all pass

- [ ] **Step 5: Run Miri**

Run: `cargo +nightly miri test -p mantis-queue --all-features`
Expected: no UB

- [ ] **Step 6: Commit**

```bash
git add crates/queue/src/copy_ring/engine.rs
git commit -m "feat(queue): add batch push_batch/pop_batch to CopyRingEngine"
```

---

## Task 5: RawRingCopy Public Handle + Presets + Split Handles

**Files:**
- Modify: `crates/queue/src/copy_ring/mod.rs`
- Modify: `crates/queue/src/copy_ring/handle.rs`
- Modify: `crates/queue/src/presets.rs`
- Modify: `crates/queue/src/lib.rs`

- [ ] **Step 1: Write integration tests**

Create `crates/queue/tests/spsc_copy_basic.rs`:

```rust
//! Integration tests for the copy-optimized SPSC ring.
use mantis_queue::{SpscRingCopy, SpscRingCopyHeap, SpscRingCopyInstrumented};

#[test]
fn inline_push_pop() {
    let mut ring = SpscRingCopy::<u64, 8>::new();
    assert!(ring.push(&42));
    let mut out = 0u64;
    assert!(ring.pop(&mut out));
    assert_eq!(out, 42);
}

#[test]
fn inline_fill_and_drain() {
    let mut ring = SpscRingCopy::<u64, 8>::new();
    for i in 0u64..7 {
        assert!(ring.push(&i));
    }
    assert!(!ring.push(&99)); // full
    for i in 0u64..7 {
        let mut out = 0u64;
        assert!(ring.pop(&mut out));
        assert_eq!(out, i);
    }
    let mut out = 0u64;
    assert!(!ring.pop(&mut out)); // empty
}

#[test]
fn inline_batch_roundtrip() {
    let mut ring = SpscRingCopy::<u64, 1024>::new();
    let src: Vec<u64> = (0..100).collect();
    assert_eq!(ring.push_batch(&src), 100);
    let mut dst = vec![0u64; 100];
    assert_eq!(ring.pop_batch(&mut dst), 100);
    assert_eq!(src, dst);
}

#[cfg(feature = "alloc")]
#[test]
fn stress_two_thread_message48() {
    use mantis_queue::spsc_ring_copy;
    use std::thread;
    use core::hint::spin_loop;

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg48 {
        seq: u64, a: u64, b: u64, c: u64, d: u64, e: u64,
    }

    let count: u64 = if cfg!(miri) { 100 } else { 10_000_000 };
    let (tx, rx) = spsc_ring_copy::<Msg48, { 1 << 14 }>();

    let producer = thread::spawn(move || {
        for i in 0..count {
            let msg = Msg48 { seq: i, ..Msg48::default() };
            while !tx.push(&msg) { spin_loop(); }
        }
    });

    let consumer = thread::spawn(move || {
        let mut out = Msg48::default();
        for i in 0..count {
            while !rx.pop(&mut out) { spin_loop(); }
            assert_eq!(out.seq, i);
        }
    });

    producer.join().expect("producer panicked");
    consumer.join().expect("consumer panicked");
}

#[cfg(feature = "alloc")]
#[test]
fn heap_push_pop() {
    let mut ring = SpscRingCopyHeap::<u64>::with_capacity(128);
    assert!(ring.push(&42));
    let mut out = 0u64;
    assert!(ring.pop(&mut out));
    assert_eq!(out, 42);
}

#[test]
fn instrumented_counts() {
    let mut ring = SpscRingCopyInstrumented::<u64, 8>::new();
    ring.push(&1);
    ring.push(&2);
    let mut out = 0u64;
    ring.pop(&mut out);
    assert_eq!(ring.instrumentation().push_count(), 2);
    assert_eq!(ring.instrumentation().pop_count(), 1);
}

#[cfg(feature = "alloc")]
#[test]
fn split_handles_two_thread() {
    use mantis_queue::spsc_ring_copy;
    use std::thread;
    use core::hint::spin_loop;

    let count = if cfg!(miri) { 1_000 } else { 10_000 };
    let (tx, rx) = spsc_ring_copy::<u64, 1024>();

    let producer = thread::spawn(move || {
        for i in 0..count {
            while !tx.push(&i) {
                spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut out = 0u64;
        for i in 0..count {
            while !rx.pop(&mut out) {
                spin_loop();
            }
            assert_eq!(out, i);
        }
    });

    producer.join().expect("producer panicked");
    consumer.join().expect("consumer panicked");
}

#[cfg(feature = "alloc")]
#[test]
fn split_handles_batch_two_thread() {
    use mantis_queue::spsc_ring_copy;
    use std::thread;
    use core::hint::spin_loop;

    let total = if cfg!(miri) { 500 } else { 100_000 };
    let batch_size = 50;
    let (tx, rx) = spsc_ring_copy::<u64, 1024>();

    let producer = thread::spawn(move || {
        let mut sent = 0u64;
        while sent < total {
            let remaining = (total - sent) as usize;
            let n = remaining.min(batch_size);
            let batch: Vec<u64> = (sent..sent + n as u64).collect();
            let pushed = tx.push_batch(&batch);
            sent += pushed as u64;
            if pushed == 0 {
                spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut received = 0u64;
        let mut buf = vec![0u64; batch_size];
        while received < total {
            let popped = rx.pop_batch(&mut buf);
            for val in &buf[..popped] {
                assert_eq!(*val, received);
                received += 1;
            }
            if popped == 0 {
                spin_loop();
            }
        }
    });

    producer.join().expect("producer panicked");
    consumer.join().expect("consumer panicked");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mantis-queue --all-features`
Expected: FAIL — `SpscRingCopy` not found

- [ ] **Step 3: Implement RawRingCopy**

Replace `crates/queue/src/copy_ring/mod.rs`:

```rust
//! Copy-optimized SPSC ring buffer for `T: Copy` types.
//!
//! Provides SIMD-accelerated slot copies, batch push/pop, and
//! cold-path hints for sub-nanosecond hot paths.

pub(crate) mod engine;
#[cfg(feature = "alloc")]
pub(crate) mod handle;
pub(crate) mod raw;

use core::marker::PhantomData;

use mantis_core::{CopyPolicy, IndexStrategy, Instrumentation, PushPolicy};

use crate::storage::Storage;
use engine::CopyRingEngine;

/// Public handle for the copy-optimized SPSC ring.
///
/// Analogous to `RawRing` but requires `T: Copy` and provides
/// batch operations and SIMD-accelerated copies.
pub struct RawRingCopy<T: Copy, S, I, P, Instr, CP> {
    engine: CopyRingEngine<T, S, I, P, Instr, CP>,
}

impl<T, S, I, P, Instr, CP> RawRingCopy<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
    pub(crate) fn with_strategies(storage: S, instr: Instr) -> Self {
        Self {
            engine: CopyRingEngine::new(storage, instr),
        }
    }

    /// Push a value by copying it into the ring.
    /// Returns `true` on success, `false` if the ring is full.
    #[inline(always)]
    pub fn push(&mut self, value: &T) -> bool {
        self.engine.push(value)
    }

    /// Pop a value by copying it out of the ring.
    /// Returns `true` on success, `false` if the ring is empty.
    #[inline(always)]
    pub fn pop(&mut self, out: &mut T) -> bool {
        self.engine.pop(out)
    }

    /// Push a batch of values. Returns the number actually pushed.
    #[inline(always)]
    pub fn push_batch(&mut self, src: &[T]) -> usize {
        self.engine.push_batch(src)
    }

    /// Pop a batch of values. Returns the number actually popped.
    #[inline(always)]
    pub fn pop_batch(&mut self, dst: &mut [T]) -> usize {
        self.engine.pop_batch(dst)
    }

    /// Usable capacity (storage capacity minus sentinel slot).
    #[inline]
    pub fn capacity(&self) -> usize {
        self.engine.capacity()
    }

    /// Current number of items in the ring.
    #[inline]
    pub fn len(&self) -> usize {
        self.engine.len()
    }

    /// Whether the ring is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.engine.is_empty()
    }

    /// Access the instrumentation counters.
    #[inline]
    pub fn instrumentation(&self) -> &Instr {
        self.engine.instrumentation()
    }
}
```

- [ ] **Step 4: Implement split handles**

Replace `crates/queue/src/copy_ring/handle.rs`:

```rust
//! Split producer/consumer handles for the copy-optimized ring.

extern crate alloc;

use alloc::sync::Arc;
use core::marker::PhantomData;

use mantis_core::{CopyPolicy, IndexStrategy, Instrumentation, PushPolicy};

use crate::copy_ring::engine::CopyRingEngine;
use crate::storage::Storage;

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
#[allow(unsafe_code)]
unsafe impl<T, S, I, P, Instr, CP> Send
    for ProducerCopy<T, S, I, P, Instr, CP>
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
#[allow(unsafe_code)]
unsafe impl<T, S, I, P, Instr, CP> Send
    for ConsumerCopy<T, S, I, P, Instr, CP>
where
    T: Copy + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
}

impl<T, S, I, P, Instr, CP> ProducerCopy<T, S, I, P, Instr, CP>
where
    T: Copy,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
    CP: CopyPolicy<T>,
{
    #[inline(always)]
    pub fn push(&self, value: &T) -> bool {
        self.engine.push(value)
    }

    #[inline(always)]
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
    #[inline(always)]
    pub fn pop(&self, out: &mut T) -> bool {
        self.engine.pop(out)
    }

    #[inline(always)]
    pub fn pop_batch(&self, dst: &mut [T]) -> usize {
        self.engine.pop_batch(dst)
    }
}

use crate::copy_ring::raw::DefaultCopyPolicy;
use crate::storage::InlineStorage;
use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};

/// Create split producer/consumer handles for an inline copy ring.
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

#[cfg(feature = "alloc")]
use crate::storage::HeapStorage;

/// Create split producer/consumer handles for a heap copy ring.
#[cfg(feature = "alloc")]
pub fn spsc_ring_copy_heap<T: Copy + Send>(capacity: usize) -> (
    ProducerCopy<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>,
    ConsumerCopy<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>,
) {
    let engine = Arc::new(CopyRingEngine::new(HeapStorage::new(capacity), NoInstr));
    (
        ProducerCopy { engine: Arc::clone(&engine), _not_sync: PhantomData },
        ConsumerCopy { engine, _not_sync: PhantomData },
    )
}
```

- [ ] **Step 5: Add preset type aliases**

Add to `crates/queue/src/presets.rs`:

```rust
use crate::copy_ring::raw::DefaultCopyPolicy;
use crate::copy_ring::RawRingCopy;

// ─── Copy-optimized presets (T: Copy only) ──────────────────────────────

/// Stack-allocated copy-optimized SPSC ring with SIMD copy dispatch.
pub type SpscRingCopy<T, const N: usize> =
    RawRingCopy<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>;

impl<T: Copy + Send, const N: usize> SpscRingCopy<T, N> {
    pub fn new() -> Self {
        RawRingCopy::with_strategies(InlineStorage::new(), NoInstr)
    }
}

impl<T: Copy + Send, const N: usize> Default for SpscRingCopy<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Heap-allocated copy-optimized SPSC ring with SIMD copy dispatch.
#[cfg(feature = "alloc")]
pub type SpscRingCopyHeap<T> =
    RawRingCopy<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>;

#[cfg(feature = "alloc")]
impl<T: Copy + Send> SpscRingCopyHeap<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        RawRingCopy::with_strategies(HeapStorage::new(capacity), NoInstr)
    }
}

/// Copy-optimized SPSC ring with instrumentation counters.
pub type SpscRingCopyInstrumented<T, const N: usize> =
    RawRingCopy<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, CountingInstr, DefaultCopyPolicy>;

impl<T: Copy + Send, const N: usize> SpscRingCopyInstrumented<T, N> {
    pub fn new() -> Self {
        RawRingCopy::with_strategies(InlineStorage::new(), CountingInstr::new())
    }
}

impl<T: Copy + Send, const N: usize> Default for SpscRingCopyInstrumented<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 6: Update lib.rs re-exports**

Add to `crates/queue/src/lib.rs` in the re-exports section:

```rust
pub use copy_ring::RawRingCopy;
#[cfg(feature = "alloc")]
pub use copy_ring::handle::{
    spsc_ring_copy, spsc_ring_copy_heap,
    ConsumerCopy, ProducerCopy,
};
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: all pass including new integration tests

- [ ] **Step 8: Run Miri**

Run: `cargo +nightly miri test -p mantis-queue --all-features`
Expected: no UB (two-thread test runs with 1K items under Miri)

- [ ] **Step 9: Run clippy**

Run: `cargo clippy -p mantis-queue --all-targets --all-features -- -D warnings`
Expected: no warnings

- [ ] **Step 10: Commit**

```bash
git add crates/queue/src/copy_ring/ crates/queue/src/presets.rs crates/queue/src/lib.rs crates/queue/tests/spsc_copy_basic.rs
git commit -m "feat(queue): add RawRingCopy, presets, split handles, and integration tests"
```

---

## Task 6: Cold-Path Hints on Existing RingEngine

**Files:**
- Modify: `crates/queue/src/engine.rs`

- [ ] **Step 1: Add slow_full/slow_empty functions to engine.rs**

Add before the `impl` block in `crates/queue/src/engine.rs`:

```rust
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
```

- [ ] **Step 2: Use cold functions in try_push/try_pop error paths**

In `try_push`, replace the `return Err(PushError::Full(value));` line with:

```rust
                #[cfg(feature = "nightly")]
                core::hint::cold_path();
                self.instr.on_push_full();
                return slow_push_full(value);
```

In `try_pop`, replace the `return Err(QueueError::Empty);` line with:

```rust
                #[cfg(feature = "nightly")]
                core::hint::cold_path();
                self.instr.on_pop_empty();
                return slow_pop_empty();
```

- [ ] **Step 3: Run all tests**

Run: `cargo test -p mantis-queue --all-features`
Expected: all existing + new tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/queue/src/engine.rs
git commit -m "perf(queue): add cold-path hints to RingEngine"
```

---

## Task 7: Test Message Types + Benchmarks

**Files:**
- Create: `crates/bench/src/messages.rs`
- Modify: `crates/bench/src/lib.rs`
- Modify: `crates/bench/src/workloads.rs`
- Modify: `crates/bench/benches/spsc_mantis.rs`
- Modify: `crates/queue/examples/asm_shim.rs`

- [ ] **Step 1: Create message types**

Create `crates/bench/src/messages.rs`:

```rust
//! Realistic financial message types for benchmarks.
//!
//! These hit the exact SIMD kernel sizes (48, 64 bytes) to exercise
//! the copy-optimized ring's hot paths.

use core::mem::size_of;

#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Message48 {
    pub timestamp: u64,
    pub symbol_id: u32,
    pub side: u16,
    pub flags: u16,
    pub price: i64,
    pub quantity: i64,
    pub order_id: i64,
    pub sequence: u64,
}

const _: () = assert!(size_of::<Message48>() == 48);

#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Message64 {
    pub timestamp: u64,
    pub symbol_id: u32,
    pub side: u16,
    pub flags: u16,
    pub price: i64,
    pub quantity: i64,
    pub order_id: i64,
    pub sequence: u64,
    pub venue_id: u32,
    pub _pad: u32,
    pub client_order_id: u64,
}

const _: () = assert!(size_of::<Message64>() == 64);

/// Deterministic test message for reproducible benchmarks.
pub fn make_msg48(i: u64) -> Message48 {
    Message48 {
        timestamp: i,
        symbol_id: i as u32,
        side: (i & 1) as u16,
        flags: (i & 0x3) as u16,
        price: i as i64 * 10,
        quantity: i as i64 * 100,
        order_id: i as i64 * 1000,
        sequence: i,
    }
}

/// Deterministic test message for reproducible benchmarks.
pub fn make_msg64(i: u64) -> Message64 {
    Message64 {
        timestamp: i,
        symbol_id: i as u32,
        side: (i & 1) as u16,
        flags: (i & 0x3) as u16,
        price: i as i64 * 10,
        quantity: i as i64 * 100,
        order_id: i as i64 * 1000,
        sequence: i,
        venue_id: (i & 0xFF) as u32,
        _pad: 0,
        client_order_id: i * 10_000,
    }
}
```

- [ ] **Step 2: Add module to bench lib.rs**

Add to `crates/bench/src/lib.rs`:

```rust
pub mod messages;
```

- [ ] **Step 3: Add batch workload functions**

Add to `crates/bench/src/workloads.rs`:

```rust
use mantis_queue::SpscRingCopy;

/// Single push+pop for copy ring.
pub fn single_item_copy<T: Copy + Send + Default, const N: usize>(
    ring: &mut SpscRingCopy<T, N>,
    values: &[T],
) {
    let mut out = T::default();
    for val in values {
        let _ = ring.push(val);
        let _ = ring.pop(&mut out);
    }
}

/// Burst of single push+pop for copy ring.
pub fn burst_copy<T: Copy + Send + Default, const N: usize>(
    ring: &mut SpscRingCopy<T, N>,
    values: &[T],
    burst_size: usize,
) {
    let mut out = T::default();
    for chunk in values.chunks(burst_size) {
        for val in chunk {
            let _ = ring.push(val);
        }
        for _ in 0..chunk.len() {
            let _ = ring.pop(&mut out);
        }
    }
}

/// Batch push + batch pop for copy ring.
pub fn batch_copy<T: Copy + Send + Default, const N: usize>(
    ring: &mut SpscRingCopy<T, N>,
    values: &[T],
    batch_size: usize,
) {
    let mut out = vec![T::default(); batch_size];
    for chunk in values.chunks(batch_size) {
        let pushed = ring.push_batch(chunk);
        let _ = ring.pop_batch(&mut out[..pushed]);
    }
}
```

- [ ] **Step 4: Add copy-ring benchmarks to spsc_mantis.rs**

Add a new benchmark group function and call it from `criterion_main!`. The implementer should read `crates/bench/benches/spsc_mantis.rs` to understand the existing `run_bench` / `export_report` pattern, then add these benchmarks following the same structure:

```rust
use mantis_bench::messages::{make_msg48, make_msg64, Message48, Message64};
use mantis_bench::workloads::{single_item_copy, burst_copy, batch_copy};
use mantis_queue::{SpscRing, SpscRingCopy};

fn copy_ring_benches(c: &mut MantisC) {
    let mut descs = Vec::new();

    // --- Copy ring: single push+pop ---
    {
        let mut ring = SpscRingCopy::<u64, 1024>::new();
        descs.push(run_bench(c, "copy/single/u64", |b| {
            b.iter(|| {
                let _ = ring.push(black_box(&42u64));
                let mut out = 0u64;
                let _ = ring.pop(black_box(&mut out));
            });
        }));
    }
    {
        let mut ring = SpscRingCopy::<Message48, 1024>::new();
        let msg = make_msg48(1);
        descs.push(run_bench(c, "copy/single/msg48", |b| {
            b.iter(|| {
                let _ = ring.push(black_box(&msg));
                let mut out = Message48::default();
                let _ = ring.pop(black_box(&mut out));
            });
        }));
    }
    {
        let mut ring = SpscRingCopy::<Message64, 1024>::new();
        let msg = make_msg64(1);
        descs.push(run_bench(c, "copy/single/msg64", |b| {
            b.iter(|| {
                let _ = ring.push(black_box(&msg));
                let mut out = Message64::default();
                let _ = ring.pop(black_box(&mut out));
            });
        }));
    }

    // --- Copy ring: burst (loop of single push+pop) ---
    {
        let mut ring = SpscRingCopy::<Message48, 1024>::new();
        let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
        descs.push(run_bench(c, "copy/burst/100/msg48", |b| {
            b.iter(|| burst_copy(&mut ring, black_box(&msgs), 100));
        }));
    }
    {
        let mut ring = SpscRingCopy::<Message48, 2048>::new();
        let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
        descs.push(run_bench(c, "copy/burst/1000/msg48", |b| {
            b.iter(|| burst_copy(&mut ring, black_box(&msgs), 1000));
        }));
    }

    // --- Copy ring: batch push+pop ---
    {
        let mut ring = SpscRingCopy::<Message48, 1024>::new();
        let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
        descs.push(run_bench(c, "copy/batch/100/msg48", |b| {
            b.iter(|| batch_copy(&mut ring, black_box(&msgs), 100));
        }));
    }
    {
        let mut ring = SpscRingCopy::<Message48, 2048>::new();
        let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
        descs.push(run_bench(c, "copy/batch/1000/msg48", |b| {
            b.iter(|| batch_copy(&mut ring, black_box(&msgs), 1000));
        }));
    }

    // --- General ring: Message comparison baselines ---
    {
        let mut ring = SpscRing::<Message48, 1024>::new();
        let msg = make_msg48(1);
        descs.push(run_bench(c, "general/single/msg48", |b| {
            b.iter(|| {
                let _ = ring.try_push(black_box(msg));
                let _ = black_box(ring.try_pop());
            });
        }));
    }
    {
        let mut ring = SpscRing::<Message64, 1024>::new();
        let msg = make_msg64(1);
        descs.push(run_bench(c, "general/single/msg64", |b| {
            b.iter(|| {
                let _ = ring.try_push(black_box(msg));
                let _ = black_box(ring.try_pop());
            });
        }));
    }

    export_report(&descs, "SpscRingCopy (SIMD)", "copy-ring", vec![]);
}
```

Add `copy_ring_benches` to the existing `criterion_group!` and `criterion_main!` macros alongside the existing benchmark groups.

- [ ] **Step 5: Add copy-ring ASM shims**

Add to `crates/queue/examples/asm_shim.rs`:

```rust
use mantis_queue::SpscRingCopy;

#[inline(never)]
pub fn spsc_copy_push_u64(ring: &mut SpscRingCopy<u64, 1024>, val: &u64) -> bool {
    ring.push(val)
}

#[inline(never)]
pub fn spsc_copy_pop_u64(ring: &mut SpscRingCopy<u64, 1024>, out: &mut u64) -> bool {
    ring.pop(out)
}
```

- [ ] **Step 6: Verify benchmarks compile**

Run: `cargo bench -p mantis-bench --bench spsc_mantis -- --test`
Expected: benchmarks compile and run in test mode (no actual measurement)

- [ ] **Step 7: Commit**

```bash
git add crates/bench/src/messages.rs crates/bench/src/lib.rs crates/bench/src/workloads.rs crates/bench/benches/spsc_mantis.rs crates/queue/examples/asm_shim.rs
git commit -m "feat(bench): add Message48/64 types and copy-ring benchmarks"
```

---

## Task 8: Verification (Differential + Property Tests)

**Files:**
- Modify: `crates/verify/src/spsc_diff.rs`
- Modify: `crates/verify/src/spsc_props.rs`

- [ ] **Step 1: Add copy-ring differential test**

Add to `crates/verify/src/spsc_diff.rs`:

```rust
/// Verify SpscRingCopy produces identical output to SpscRing
/// for the same push/pop sequence.
#[test]
fn copy_vs_general_fixed() {
    use mantis_queue::{SpscRing, SpscRingCopy};

    let mut general = SpscRing::<u64, 16>::new();
    let mut copy = SpscRingCopy::<u64, 16>::new();

    let ops = vec![
        true, true, true, false, false,
        true, true, true, true, false,
        false, false, true, false,
    ];

    let mut gen_out = Vec::new();
    let mut copy_out = Vec::new();
    let mut val = 0u64;

    for &is_push in &ops {
        if is_push {
            let gen_ok = general.try_push(val).is_ok();
            let copy_ok = copy.push(&val);
            assert_eq!(gen_ok, copy_ok, "push diverged at val={val}");
            val += 1;
        } else {
            match general.try_pop() {
                Ok(v) => gen_out.push(v),
                Err(_) => {}
            }
            let mut out = 0u64;
            if copy.pop(&mut out) {
                copy_out.push(out);
            }
        }
    }

    assert_eq!(gen_out, copy_out);
}
```

- [ ] **Step 2: Add copy-ring batch property test**

Add to `crates/verify/src/spsc_props.rs`:

```rust
/// Batch push followed by batch pop preserves FIFO ordering.
#[test]
fn copy_batch_fifo_ordering() {
    bolero::check!().with_type::<Vec<u8>>().for_each(|data| {
        if data.is_empty() { return; }

        let mut ring = mantis_queue::SpscRingCopy::<u8, 256>::new();
        let pushed = ring.push_batch(data);
        let mut out = vec![0u8; pushed];
        let popped = ring.pop_batch(&mut out);

        assert_eq!(pushed, popped);
        assert_eq!(&data[..pushed], &out[..popped]);
    });
}

/// Batch push never exceeds capacity.
#[test]
fn copy_batch_respects_capacity() {
    bolero::check!().with_type::<Vec<u8>>().for_each(|data| {
        let mut ring = mantis_queue::SpscRingCopy::<u8, 16>::new();
        let pushed = ring.push_batch(data);
        assert!(pushed <= ring.capacity());
        assert!(pushed <= data.len());
    });
}
```

- [ ] **Step 3: Run verification tests**

Run: `cargo test -p mantis-verify --all-features`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/verify/src/spsc_diff.rs crates/verify/src/spsc_props.rs
git commit -m "test(verify): add copy-ring differential and property tests"
```

---

## Task Order and Dependencies

```
Task 1: CopyPolicy trait + nightly feature ──┬─→ Task 2: SIMD kernels
                                              │         │
                                              │         ├─→ Task 3: Engine (single push/pop)
                                              │         │         │
                                              │         │         ├─→ Task 4: Batch push/pop
                                              │         │         │         │
                                              │         │         │         ├─→ Task 5: Public handle + presets
                                              │         │         │         │         │
                                              ├─→ Task 6: Cold-path hints  │         ├─→ Task 7: Benchmarks
                                              │  (existing engine)         │         │
                                                                           │         ├─→ Task 8: Verification
```

Tasks 1-5 are strictly sequential (each builds on the previous).
Task 6 depends on Task 1 (needs `nightly` feature flag) but is independent of Tasks 2-5.
Tasks 7 and 8 depend on Task 5 and can run in parallel.
