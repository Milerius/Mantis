# SPSC Copy-Optimized Ring Buffer — Phase 2 Design

## Goal

Add a `T: Copy` optimized SPSC ring buffer with pluggable SIMD copy kernels, batch push/pop, and cold-path hints alongside the existing general-purpose ring. Benchmark both side-by-side.

## Architecture

A new `copy_ring` module inside `mantis-queue` provides a copy-optimized engine parallel to the existing `RingEngine`. It shares the same strategy traits (`IndexStrategy`, `PushPolicy`, `Instrumentation`) from `mantis-core` but adds a `CopyPolicy<T>` trait for pluggable slot copy strategies. Platform-specific SIMD kernels (SSE2 on x86_64, NEON on aarch64) handle hot-path message sizes (16/32/48/64 bytes) with compile-time size dispatch. A `nightly` feature flag gates `likely`/`unlikely`/`cold_path` intrinsics; stable builds rely on `#[cold]` function attributes.

## Tech Stack

- Rust stable (with optional `nightly` feature)
- `core::arch::x86_64` (SSE2 intrinsics)
- `core::arch::aarch64` (NEON intrinsics)
- `core::hint::{likely, unlikely, cold_path}` (nightly only)
- `generic_const_exprs` (nightly only — enables `CopyDispatcher<T, {size_of::<T>()}>`)
- Criterion (benchmarks)

---

## 1. CopyPolicy Trait

**Crate:** `mantis-core`
**File:** `crates/core/src/lib.rs`

New strategy trait alongside `IndexStrategy`, `PushPolicy`, `Instrumentation`:

```rust
/// Copy strategy for SPSC ring slot operations.
///
/// Implementations are zero-sized types used for static dispatch only.
/// No instance is ever constructed — all methods are associated functions.
/// The default implementation uses compile-time size dispatch to select
/// SIMD kernels for common message sizes.
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

No `Send + Sync + 'static` supertrait bounds — `CopyPolicy` is a ZST marker for static dispatch. The `PhantomData<CP>` in the engine carries the type without ever constructing an instance.

**Concrete implementation:** `DefaultCopyPolicy` (a zero-sized struct) in `mantis-queue` delegates to the compile-time size dispatcher.

## 2. SIMD Copy Kernels

**Crate:** `mantis-queue`
**File:** `crates/queue/src/copy_ring/raw/simd.rs`

All SIMD unsafe code lives under the `raw` submodule to comply with the project unsafe isolation policy. The module structure is `copy_ring/raw/mod.rs` (safe wrappers) and `copy_ring/raw/simd.rs` (platform intrinsics).

### Compile-Time Size Dispatch

```rust
pub struct CopyDispatcher<T, const N: usize>(PhantomData<T>);

impl<T: Copy, const N: usize> CopyDispatcher<T, N> {
    #[inline(always)]
    pub unsafe fn copy(dst: *mut T, src: *const T) {
        if N <= 8 {
            ptr::copy_nonoverlapping(src, dst, 1);
        } else if N == 16 { copy_16(dst.cast(), src.cast()) }
        else if N == 32 { copy_32(dst.cast(), src.cast()) }
        else if N == 48 { copy_48(dst.cast(), src.cast()) }
        else if N == 64 { copy_64(dst.cast(), src.cast()) }
        else if N < 32 { copy_bucket::<16, N>(dst.cast(), src.cast()) }
        else if N < 48 { copy_bucket::<32, N>(dst.cast(), src.cast()) }
        else if N < 64 { copy_bucket::<48, N>(dst.cast(), src.cast()) }
        else { ptr::copy_nonoverlapping(src, dst, 1) }
    }
}
```

Since `N` is a const generic, all branches are evaluated at compile time — dead branches are eliminated, so the generated code for e.g. `Message48` contains only the `copy_48` call.

### Platform Kernels

**x86_64 — SSE2:**

```rust
#[cfg(target_arch = "x86_64")]
mod x86 {
    use core::arch::x86_64::*;

    #[inline(always)]
    pub unsafe fn load128(src: *const u8) -> __m128i {
        _mm_loadu_si128(src as *const __m128i)
    }

    #[inline(always)]
    pub unsafe fn store128(dst: *mut u8, v: __m128i) {
        _mm_storeu_si128(dst as *mut __m128i, v);
    }
}
```

**aarch64 — NEON:**

```rust
#[cfg(target_arch = "aarch64")]
mod arm {
    use core::arch::aarch64::*;

    #[inline(always)]
    pub unsafe fn load128(src: *const u8) -> uint8x16_t {
        vld1q_u8(src)
    }

    #[inline(always)]
    pub unsafe fn store128(dst: *mut u8, v: uint8x16_t) {
        vst1q_u8(dst, v);
    }
}
```

### Macro-Generated Exact-Size Kernels

```rust
macro_rules! define_copy_exact {
    ($name:ident, $bytes:expr, $chunks:expr) => {
        #[inline(always)]
        unsafe fn $name(dst: *mut u8, src: *const u8) {
            let mut i = 0usize;
            while i < $chunks {
                let v = load128(src.add(i * 16));
                store128(dst.add(i * 16), v);
                i += 1;
            }
        }
    };
}

define_copy_exact!(copy_16, 16, 1);  // 1x 128-bit load/store
define_copy_exact!(copy_32, 32, 2);  // 2x 128-bit load/store
define_copy_exact!(copy_48, 48, 3);  // 3x 128-bit load/store
define_copy_exact!(copy_64, 64, 4);  // 4x 128-bit load/store
```

The `load128`/`store128` functions resolve to the platform-specific intrinsic via `#[cfg(target_arch)]`. On unsupported architectures, the dispatcher falls through to `ptr::copy_nonoverlapping`.

### Bucket Fallback (In-Between Sizes)

For sizes like 20 or 40 bytes that don't hit an exact kernel, using const generic `N`:

```rust
/// Copy `BASE` bytes via SIMD kernel, then scalar for the remainder.
/// Both `BASE` and `N` are const — no runtime parameters.
#[inline(always)]
unsafe fn copy_bucket<const BASE: usize, const N: usize>(
    dst: *mut u8,
    src: *const u8,
) {
    // Copy the aligned prefix via SIMD
    match BASE {
        16 => copy_16(dst, src),
        32 => copy_32(dst, src),
        48 => copy_48(dst, src),
        _ => {}
    }
    // Scalar remainder (N - BASE bytes)
    ptr::copy_nonoverlapping(src.add(BASE), dst.add(BASE), N - BASE);
}
```

All parameters are compile-time constants — no runtime `n: usize`.

## 3. Copy-Optimized Engine

**Crate:** `mantis-queue`
**File:** `crates/queue/src/copy_ring/engine.rs`

### Index Convention

`CopyRingEngine` follows the **same convention** as the existing `RingEngine`:
- **Producer** owns `head` (write pointer) and caches `tail` in `tail_cached`
- **Consumer** owns `tail` (read pointer) and caches `head` in `head_cached`
- Indices are **bounded** — always `< capacity` after `I::wrap()`, same as `RingEngine`
- One sentinel slot: `capacity - 1` usable slots (same as `RingEngine`)

### Structure

```rust
pub(crate) struct CopyRingEngine<T: Copy, S, I, P, Instr, CP> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    tail_cached: CachePadded<Cell<usize>>,
    head_cached: CachePadded<Cell<usize>>,
    storage: S,
    instr: Instr,
    _marker: PhantomData<(T, I, P, CP)>,
}
```

### Key Differences from `RingEngine`

| Aspect | `RingEngine` | `CopyRingEngine` |
|---|---|---|
| Bound on `T` | any | `T: Copy` |
| Push API | `try_push(value: T) -> Result<(), PushError<T>>` | `push(&self, value: &T) -> bool` |
| Pop API | `try_pop() -> Result<T, QueueError>` | `pop(&self, out: &mut T) -> bool` |
| Slot write | `ptr::write` | `CopyPolicy::copy_in` |
| Slot read | `ptr::read` | `CopyPolicy::copy_out` |
| Drop impl | `drop_range` walks occupied slots | No-op (T: Copy, no destructors) |
| Batch ops | None | `push_batch`, `pop_batch` |
| Error type | `PushError<T>` / `QueueError` | `bool` (caller still has value since T: Copy) |

### Push (Single)

Follows the same head=producer, tail=consumer convention as `RingEngine`. On nightly, `unlikely` wraps the cache-miss branch; on stable, the branch structure alone guides prediction (early return = unlikely).

```rust
#[inline(always)]
pub fn push(&self, value: &T) -> bool {
    let head = self.head.load(Ordering::Relaxed);
    let next_head = I::wrap(head + 1, self.storage.capacity());

    // Cache miss: check remote tail
    // On nightly: if unlikely(next_head == self.tail_cached.get())
    // On stable: bare if (early-return structure is naturally unlikely)
    if next_head == self.tail_cached.get() {
        let tail = self.tail.load(Ordering::Acquire);
        self.tail_cached.set(tail);
        if next_head == tail {
            return slow_full();
        }
    }

    unsafe { CP::copy_in(self.storage.slot_ptr(head).cast(), value as *const T) };
    self.head.store(next_head, Ordering::Release);
    self.instr.on_push();
    true
}
```

### Pop (Single)

```rust
#[inline(always)]
pub fn pop(&self, out: &mut T) -> bool {
    let tail = self.tail.load(Ordering::Relaxed);

    // Cache miss: check remote head
    if tail == self.head_cached.get() {
        let head = self.head.load(Ordering::Acquire);
        self.head_cached.set(head);
        if tail == head {
            return slow_empty();
        }
    }

    unsafe { CP::copy_out(out as *mut T, self.storage.slot_ptr(tail).cast()) };
    let next_tail = I::wrap(tail + 1, self.storage.capacity());
    self.tail.store(next_tail, Ordering::Release);
    self.instr.on_pop();
    true
}
```

### Batch Push

Indices are bounded (always masked via `I::wrap`), so the batch loop must wrap each index. Free-space uses `capacity() - 1` (usable slots, accounting for the sentinel).

```rust
#[inline(always)]
pub fn push_batch(&self, src: &[T]) -> usize {
    if src.is_empty() { return 0; }

    let head = self.head.load(Ordering::Relaxed);
    let cached_tail = self.tail_cached.get();
    let cap = self.storage.capacity();
    let usable = cap - 1; // sentinel slot

    // Compute free slots: usable - current_len
    // current_len = (head - cached_tail) mod cap
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
        if free == 0 { return 0; }
    }

    let n = src.len().min(free);
    let mut idx = head;
    for item in &src[..n] {
        unsafe { CP::copy_in(self.storage.slot_ptr(idx).cast(), item as *const T) };
        idx = I::wrap(idx + 1, cap);
    }

    // Single atomic publish for all n items
    self.head.store(idx, Ordering::Release);
    n
}
```

### Batch Pop

Same pattern: compute available items from `tail` to `head_cached`, loop with `I::wrap` per element, single atomic store of final `tail`.

```rust
#[inline(always)]
pub fn pop_batch(&self, dst: &mut [T]) -> usize {
    if dst.is_empty() { return 0; }

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
        if avail == 0 { return 0; }
    }

    let n = dst.len().min(avail);
    let mut idx = tail;
    for out in &mut dst[..n] {
        unsafe { CP::copy_out(out as *mut T, self.storage.slot_ptr(idx).cast()) };
        idx = I::wrap(idx + 1, cap);
    }

    self.tail.store(idx, Ordering::Release);
    n
}
```

## 4. Public Handle: `RawRingCopy`

**File:** `crates/queue/src/copy_ring/mod.rs`

`RawRingCopy` is the public-facing wrapper around `CopyRingEngine`, analogous to `RawRing`:

```rust
pub struct RawRingCopy<T: Copy, S, I, P, Instr, CP> {
    engine: CopyRingEngine<T, S, I, P, Instr, CP>,
}
```

**Public methods:**

| Method | Signature | Description |
|---|---|---|
| `new` | `fn new() -> Self` (InlineStorage) | Construct with inline storage |
| `with_capacity` | `fn with_capacity(cap: usize) -> Self` (HeapStorage) | Construct with heap storage |
| `push` | `fn push(&self, value: &T) -> bool` | Single-element push |
| `pop` | `fn pop(&self, out: &mut T) -> bool` | Single-element pop |
| `push_batch` | `fn push_batch(&self, src: &[T]) -> usize` | Batch push, returns count pushed |
| `pop_batch` | `fn pop_batch(&self, dst: &mut [T]) -> usize` | Batch pop, returns count popped |
| `capacity` | `fn capacity(&self) -> usize` | Usable capacity (storage - 1) |
| `len` | `fn len(&self) -> usize` | Current queue depth |
| `is_empty` | `fn is_empty(&self) -> bool` | `len() == 0` |
| `instrumentation` | `fn instrumentation(&self) -> &Instr` | Access instrumentation counters |

**Preset type aliases** in `crates/queue/src/presets.rs`:

```rust
pub type SpscRingCopy<T, const N: usize> =
    RawRingCopy<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>;

pub type SpscRingCopyHeap<T> =
    RawRingCopy<T, HeapStorage<T>, Pow2Masked, ImmediatePush, NoInstr, DefaultCopyPolicy>;

pub type SpscRingCopyInstrumented<T, const N: usize> =
    RawRingCopy<T, InlineStorage<T, N>, Pow2Masked, ImmediatePush, CountingInstr, DefaultCopyPolicy>;
```

### Split Handles (behind `alloc` feature)

**File:** `crates/queue/src/copy_ring/handle.rs`

```rust
pub fn spsc_ring_copy<T: Copy, const N: usize>() -> (ProducerCopy<T, ...>, ConsumerCopy<T, ...>)
pub fn spsc_ring_copy_heap<T: Copy>(cap: usize) -> (ProducerCopy<T, ...>, ConsumerCopy<T, ...>)
```

**`ProducerCopy<T, S, I, P, Instr, CP>`:**
- Wraps `Arc<CopyRingEngine<...>>`
- Exposes `push`, `push_batch`
- `Send` but `!Sync` (via `PhantomData<*const ()>`)

**`ConsumerCopy<T, S, I, P, Instr, CP>`:**
- Wraps `Arc<CopyRingEngine<...>>`
- Exposes `pop`, `pop_batch`
- `Send` but `!Sync`

Same `unsafe impl Send` justification as existing `Producer`/`Consumer`: SPSC discipline guarantees disjoint access — producer touches head/tail_cached, consumer touches tail/head_cached.

## 5. Cold-Path Hints

**Feature flag:** `nightly` in `crates/queue/Cargo.toml`

```toml
[features]
nightly = []
```

On nightly, the crate enables `#![cfg_attr(feature = "nightly", feature(likely_unlikely, generic_const_exprs))]`. This gates:
- `core::hint::{likely, unlikely, cold_path}` — branch hints
- `generic_const_exprs` — enables `CopyDispatcher<T, {size_of::<T>()}>` for compile-time size dispatch

On stable, `DefaultCopyPolicy` uses a `match size_of::<T>()` dispatch instead of const generics — LLVM still eliminates dead branches since `size_of` is known at monomorphization. The SIMD kernels work on both stable and nightly.

**Usage pattern:** On nightly, `likely`/`unlikely`/`cold_path` are used directly from `core::hint` at the branch sites in push/pop. On stable, these calls are absent — the code relies on:

1. `#[cold]` attribute on `slow_full()` / `slow_empty()` functions (works on stable)
2. Branch structure (early return = unlikely path)

```rust
// Slow-path functions — #[cold] works on stable
#[cold]
#[inline(never)]
fn slow_full() -> bool {
    false
}

#[cold]
#[inline(never)]
fn slow_empty() -> bool {
    false
}
```

On nightly, `cold_path()` is called at the branch decision site inside the hot function (not inside the cold function), where it can actually influence the optimizer:

```rust
// Inside push(), nightly only:
if next_head == tail {
    #[cfg(feature = "nightly")]
    core::hint::cold_path();
    return slow_full();
}
```

**Applied to both engines:** The existing `RingEngine` also benefits from `#[cold]` on its error paths. This is a non-breaking improvement.

## 6. Test Message Types

**Location:** `crates/bench/src/messages.rs` (bench-only, not SDK types)

```rust
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
```

`make_msg48(i: u64) -> Message48` and `make_msg64(i: u64) -> Message64` helpers for deterministic test data.

These types are also used in `crates/queue/` tests and `crates/verify/` tests (defined locally or behind `#[cfg(test)]`).

## 7. Benchmarks

**File:** `crates/bench/benches/spsc_mantis.rs` (extended)

Side-by-side comparison using the existing `bench_runner` infrastructure:

| Workload | `SpscRing` | `SpscRingCopy` |
|---|---|---|
| single push+pop u64 | existing | new |
| single push+pop Message48 | new | new |
| single push+pop Message64 | new | new |
| burst 100 Message48 (loop) | new | new |
| burst 1000 Message48 (loop) | new | new |
| batch 100 Message48 | — | new |
| batch 1000 Message48 | — | new |

"burst" = loop of single push/pop. "batch" = `push_batch`/`pop_batch`.

JSON export includes both implementations for cross-hardware comparison.

## 8. File Structure

### New Files

| File | Purpose |
|---|---|
| `crates/queue/src/copy_ring/mod.rs` | Module root, `RawRingCopy` public handle, re-exports |
| `crates/queue/src/copy_ring/engine.rs` | `CopyRingEngine` — push, pop, batch |
| `crates/queue/src/copy_ring/raw/mod.rs` | Safe wrappers for unsafe slot + SIMD operations |
| `crates/queue/src/copy_ring/raw/simd.rs` | `CopyDispatcher`, platform kernels, `DefaultCopyPolicy` |
| `crates/queue/src/copy_ring/handle.rs` | `ProducerCopy`, `ConsumerCopy` split handles (behind `alloc`) |
| `crates/bench/src/messages.rs` | `Message48`, `Message64`, helpers |
| `crates/queue/tests/spsc_copy_basic.rs` | Integration + stress tests for copy ring |

### Modified Files

| File | Change |
|---|---|
| `crates/core/src/lib.rs` | Add `CopyPolicy` trait |
| `crates/queue/src/lib.rs` | Add `mod copy_ring`, re-export new presets |
| `crates/queue/src/presets.rs` | Add Copy preset type aliases |
| `crates/queue/src/engine.rs` | Add `#[cold]` to error paths, `cold_path` on nightly |
| `crates/queue/Cargo.toml` | Add `nightly` feature |
| `crates/bench/src/lib.rs` | Add `pub mod messages` |
| `crates/bench/benches/spsc_mantis.rs` | Add Copy-ring + Message benchmarks |
| `crates/bench/src/workloads.rs` | Add batch workload functions |
| `crates/queue/examples/asm_shim.rs` | Add Copy-ring shims for ASM inspection |

## 9. Testing Strategy

| Layer | What | Where |
|---|---|---|
| Unit tests | CopyDispatcher correctness for each size bucket | `crates/queue/src/copy_ring/raw/simd.rs` |
| Unit tests | CopyRingEngine push/pop/batch single-threaded | `crates/queue/src/copy_ring/engine.rs` |
| Integration | Two-thread FIFO ordering with Message48 | `crates/queue/tests/spsc_copy_basic.rs` |
| Stress | 10M items, 2 threads, Message48 | `crates/queue/tests/spsc_copy_basic.rs` |
| Miri | All tests under Miri (no SIMD — falls back to scalar) | CI |
| Bolero | Property tests: FIFO, len invariant, batch semantics | `crates/verify/` |
| Differential | `SpscRingCopy` vs `SpscRing` — same sequences, same output | `crates/verify/` |
| Kani | Bounded model checking for batch operations | `crates/verify/` |
| ASM inspection | Verify SIMD kernels appear in generated assembly | `scripts/check-asm.sh` |

## 10. Safety

All unsafe code lives in `raw` submodules, following the project policy:

- **`crates/queue/src/copy_ring/raw/mod.rs`** — Safe wrappers (`write_slot_copy`, `read_slot_copy`) with `// SAFETY:` comments
- **`crates/queue/src/copy_ring/raw/simd.rs`** — `#![allow(unsafe_code)]` at module level. Contains `CopyDispatcher`, platform intrinsic wrappers (`load128`/`store128`), exact-size kernels, `DefaultCopyPolicy` impl
- **`crates/queue/src/copy_ring/raw/mod.rs`** — `unsafe impl Sync` for `CopyRingEngine` with same justification as `RingEngine` (SPSC discipline: producer touches head/tail_cached, consumer touches tail/head_cached — disjoint access). Lives in `raw/` per unsafe isolation policy, matching the existing `RingEngine` pattern.

Crate root `#![deny(unsafe_code)]` unchanged. Only `copy_ring/raw/` carries `#![allow(unsafe_code)]`.

## 11. Non-Goals (YAGNI)

- No contiguous memcpy for wrap-around batches (per-element SIMD is fast enough)
- No AVX2/AVX-512 kernels (SSE2 + NEON covers the common case)
- No async/await integration
- No custom allocator support
- No runtime SIMD detection (compile-time only via `target_arch`)
- No changes to existing `SpscRing` API (backward compatible)
