# SPSC Ring Buffer Optimization

## Goal

Reduce single-op latency and increase sustained throughput across all three SPSC ring implementations (inline, copy, general) through software prefetch, contiguous batch copy, and codegen verification.

## Context

### Current Baseline

The Mantis SPSC ring buffer already implements the key optimizations from the literature:

- **Unmasked indices** with power-of-2 bitwise AND wrapping (Snellman's recommended approach)
- **128-byte cache-line padding** on head/tail via `CachePadded<T>` (covers Intel 64B and Apple Silicon 128B)
- **Local cached indices** (`tail_cached`, `head_cached`) — eliminates atomic contention in the common case
- **Acquire/Release ordering** — no SeqCst; Relaxed on owned index reads
- **Cold-path hints** — `#[cold]` + `#[inline(never)]` on full/empty slow paths
- **SIMD copy dispatch** — compile-time kernel selection for 16-64 byte types (SSE2/NEON)
- **Batch operations** — `push_batch`/`pop_batch` on Copy ring

### Codegen Analysis

Assembly inspection (`cargo asm` on `asm_shim`) reveals:

**Push hot path (aarch64, u64):** 8 instructions from entry to `ret`

```asm
ldr x8, [x0, #8192]         ; head = Relaxed load
add w9, w8, #1               ; next_head = head + 1
and x9, x9, #0x3ff           ; wrap (branchless AND)
ldr x10, [x0, #8448]         ; tail_cached load
cmp x9, x10                  ; branch 1: cache hit?
b.ne LBB_fast                ; → almost always taken
; ... cache miss path (rare) ...
LBB_fast:
str x10, [x0, x8, lsl #3]   ; write slot
stlr x9, [x8_addr]          ; Release store head
ret
```

- 2 branches, both well-predicted (not-full is the common case)
- No branchless improvement possible — cached indices already eliminate contention branches
- Index wrapping is already branchless (`and`)

### Literature Research

Sources studied (mratsim/weave SPSC research + linked papers):

| Technique | Source | Our Status |
|---|---|---|
| Power-of-2 masking | Snellman, Agner Fog | Done |
| Cached indices | psy-lob-saw series | Done |
| Acquire/Release (no SeqCst) | C++ SPSC (kjellkod) | Done |
| Cache-line padding | psy-lob-saw, MCRingBuffer | Done (128B) |
| Software prefetch | MCRingBuffer, LMAX Disruptor | **Not yet** |
| Contiguous batch copy | B-Queue, Lamport | **Not yet** |
| Sentinel slot elimination | Snellman | Skipped (negligible benefit) |
| Sparse data layout | FastFlow | Skipped (cached indices sufficient) |
| Inline assembly | Constantine | Deferred (codegen already tight) |
| Hugepage allocation | rigtorp SPSCQueue | Deferred to Phase 4 |

## Architecture

### Three Ring Implementations, Two Engines

The codebase has two separate engine implementations with identical SPSC protocol:

- **`RingEngine`** (`crates/queue/src/engine.rs`): For general `T` (move semantics, `MaybeUninit` slots, Drop support)
- **`CopyRingEngine`** (`crates/queue/src/copy_ring/engine.rs`): For `T: Copy` (SIMD-optimized via `CopyPolicy`, returns `bool` not `Result`)

Both share the same structural layout:

```
[Copy]RingEngine<T, S, I, P, Instr[, CP]>
├── head: CachePadded<AtomicUsize>
├── tail: CachePadded<AtomicUsize>
├── tail_cached: CachePadded<Cell<usize>>
├── head_cached: CachePadded<Cell<usize>>
├── storage: S
└── instr: Instr
```

Public types:

- **Inline ring** (`SpscRing`): `RawRing<T, InlineStorage, ...>` — general T, takes ownership
- **Copy ring** (`SpscRingCopy`): `RawRingCopy<T, InlineStorage, ..., CopyPolicy>` — T: Copy, SIMD
- **General ring**: `RawRing` with `HeapStorage`

Prefetch changes must be added to **both** engine files independently (same pattern, same call sites).
Contiguous batch changes go into `CopyRingEngine` only (requires `T: Copy` for bulk memcpy).

## Phase 0: Codegen Baseline Capture

**Deliverable:** Extend existing `scripts/check-asm.sh` to archive hot-path assembly.

- Extend existing `scripts/check-asm.sh` (already runs `cargo asm` on `asm_shim`) with baseline archiving
- Capture baseline for: `spsc_push_u64`, `spsc_pop_u64`, `spsc_copy_push_u64`, `spsc_copy_pop_u64`
- Store instruction counts as reference for regression detection
- Target both aarch64 and x86_64 (via `--target` flag or native)

## Phase 1: Software Prefetch for Slot Access

### Problem

When a slot's cache line is cold (not in L1), the store/load on the slot stalls the pipeline. This is the dominant remaining cost for single push+pop on cross-core workloads where the slot data ping-pongs between caches.

### Design

**Existing API:** `crates/platform/src/intrinsics/compiler_hints.rs` already provides:

```rust
pub fn prefetch<T>(ptr: *const T, rw: PrefetchRW, locality: PrefetchLocality);
pub fn prefetch_large<T>(ptr: *const T, rw: PrefetchRW, locality: PrefetchLocality, max_lines: usize);
```

This API already handles x86_64 (`_mm_prefetch` with locality mapping). No new trait needed.

**Required changes to existing prefetch:**

1. **x86_64 write prefetch**: Currently ignores `PrefetchRW` (`let _ = rw;`). For write prefetch, use `_MM_HINT_ET0` (exclusive hint) to bring the cache line in Modified state, avoiding a subsequent RFO (Read For Ownership) on the store. Read prefetch continues using `_MM_HINT_T0`.

2. **aarch64 support**: Currently a no-op on non-x86_64. Add inline asm for aarch64:
   - Read: `asm!("prfm pldl1keep, [{ptr}]", ptr = in(reg) ptr, options(nostack, preserves_flags))`
   - Write: `asm!("prfm pstl1keep, [{ptr}]", ptr = in(reg) ptr, options(nostack, preserves_flags))`
   - `asm!` is stable since Rust 1.59 and works in `no_std` — gate behind `#[cfg(target_arch = "aarch64")]`, no nightly required.

| Architecture | Read Prefetch | Write Prefetch |
|---|---|---|
| x86_64 | `_mm_prefetch(ptr, _MM_HINT_T0)` | `_mm_prefetch(ptr, _MM_HINT_ET0)` |
| aarch64 (nightly) | `PRFM PLDL1KEEP` via inline asm | `PRFM PSTL1KEEP` via inline asm |
| Fallback | no-op | no-op |

**Integration into both engines:**

- Push (`RingEngine` + `CopyRingEngine`): prefetch the slot at `next_head` before writing the value
- Pop (`RingEngine` + `CopyRingEngine`): prefetch the slot at `tail` before reading the value
- Use `PrefetchRW::Write` for push, `PrefetchRW::Read` for pop
- The prefetch fires ~5-10 instructions before the actual access

**Feature flag:** `prefetch` feature in `mantis-queue` (forwarded to `mantis-platform`). Off by default for library users; enabled in bench builds.

**Risk:** Apple M-series has strong hardware prefetchers for sequential access. Software prefetch may be redundant or even harmful (polluting L1).

**Rollback criteria:** If benchmarks show >2% regression on any workload (single-op or burst) on aarch64, disable prefetch on that platform by default. The feature flag provides the rollback mechanism.

### Expected Impact

- 1-3ns improvement per single-op on cache-cold slots (cross-core scenario)
- Negligible impact on same-core (slots already hot in L1)
- No impact on instruction count (prefetch is a hint, not a barrier)

## Phase 2: Contiguous Batch Copy

### Problem

Current `push_batch`/`pop_batch` iterate per-element:

```rust
for item in &src[..n] {
    write_slot_copy(&storage, idx, item);
    idx = I::wrap(idx + 1, cap);  // AND per element
}
```

For a 1000-element burst, that's 1000 individual slot copies with per-element index wrapping, preventing the compiler from vectorizing or using bulk memcpy.

### Design

Replace per-element loop with contiguous-region calculation.

**Storage contiguity invariant:** Both `InlineStorage` (array of `UnsafeCell<MaybeUninit<T>>`) and `HeapStorage` (boxed slice) store slots contiguously in memory. The `Storage::slot_ptr(index)` returns a `*mut MaybeUninit<T>`, and `slot_ptr(i+1)` is exactly `size_of::<MaybeUninit<T>>()` bytes after `slot_ptr(i)`. This is guaranteed by Rust's array/slice layout. We rely on this for bulk `copy_nonoverlapping`.

**`push_batch` pseudocode:**

```rust
pub(crate) fn push_batch(&self, src: &[T]) -> usize {
    // ... available space calculation (unchanged) ...
    let head = self.head.load(Relaxed);
    let wrapped = I::wrap(head, cap);

    // How many slots until we hit the end of the backing array?
    let first_chunk = min(n, cap - wrapped);
    let second_chunk = n - first_chunk;

    // SAFETY: Storage slots are contiguous. slot_ptr(wrapped) through
    // slot_ptr(wrapped + first_chunk - 1) are adjacent in memory.
    unsafe {
        let dst = self.storage.slot_ptr(wrapped).cast::<T>();
        copy_nonoverlapping(src.as_ptr(), dst, first_chunk);
    }

    if second_chunk > 0 {
        unsafe {
            let dst = self.storage.slot_ptr(0).cast::<T>();
            copy_nonoverlapping(src[first_chunk..].as_ptr(), dst, second_chunk);
        }
    }

    self.head.store(I::wrap(head + n, cap), Release);
    n
}
```

**`pop_batch` pseudocode (symmetric):**

```rust
pub(crate) fn pop_batch(&self, dst: &mut [T]) -> usize {
    // ... available items calculation (unchanged) ...
    let tail = self.tail.load(Relaxed);
    let wrapped = I::wrap(tail, cap);

    let first_chunk = min(n, cap - wrapped);
    let second_chunk = n - first_chunk;

    unsafe {
        let src = self.storage.slot_ptr(wrapped).cast::<T>();
        copy_nonoverlapping(src, dst.as_mut_ptr(), first_chunk);
    }

    if second_chunk > 0 {
        unsafe {
            let src = self.storage.slot_ptr(0).cast::<T>();
            copy_nonoverlapping(src, dst[first_chunk..].as_mut_ptr(), second_chunk);
        }
    }

    self.tail.store(I::wrap(tail + n, cap), Release);
    n
}
```

**Key details:**

- `copy_nonoverlapping` compiles to `memcpy` which the compiler auto-vectorizes
- For SIMD-sized types (16-64B), we can use the existing `CopyPolicy` on each chunk
- At most 2 copies per batch (one before wrap, one after) vs N individual copies
- Available space calculation unchanged from current implementation
- The `.cast::<T>()` on `*mut MaybeUninit<T>` is safe because `MaybeUninit<T>` has the same layout as `T`

### Expected Impact

- 20-50% throughput improvement on burst workloads (burst_100, burst_1000)
- Enables auto-vectorization of the copy loop
- Reduces branch predictor pressure (2 branches vs N)

## Phase 3: Codegen Verification & Regression Gate

### Deliverable

- Run `cargo asm` post-optimization, diff against Phase 0 baseline
- Verify: prefetch instructions present, cold-path attribution intact, no register spills added
- Add instruction-count threshold to `scripts/check-asm.sh` as a soft regression gate
- Update `asm_shim.rs` with batch operation shims if needed

### Success Criteria

- Hot-path instruction count does not increase by more than 2 (prefetch adds 1-2)
- No new branches in the fast path
- Cold path remains out-of-line

## Phase 4 (Future): Hugepage Storage

Deferred to a separate spec. Overview:

- `HugePageStorage` backend using `mmap` + `MAP_HUGETLB` (Linux) / `VM_FLAGS_SUPERPAGE_SIZE_2MB` (macOS)
- Reduces TLB misses for large queues (64K+ slots spanning many 4KB pages)
- Plugs into existing `Storage<T>` trait — transparent to ring engine
- Behind `hugepages` feature flag, Linux/macOS only

## Decisions Made

| Decision | Choice | Rationale |
|---|---|---|
| Sentinel slot | Keep | <0.4% overhead at typical sizes, avoids re-verification |
| Inline assembly | Defer | Codegen already tight (8 instructions), blocks Miri |
| Sparse data | Skip | Cached indices already solve near-empty contention |
| Hugepages | Phase 4 | Orthogonal to prefetch/batch; typical sizes fit in few pages |
| Branchless rewrites | Skip | Hot path has only 2 well-predicted branches, no cmov benefit |
| Scope | All 3 rings | Prefetch added to both engines independently; batch is Copy-only |

## Testing Strategy

- **Correctness:** Existing Miri tests cover push/pop/batch. Add Miri tests for prefetch (must be no-op under Miri since Miri doesn't support prefetch intrinsics).
- **Contiguous batch correctness:**
  - Unit tests for wrap-around batch copy (push N items straddling the end of the backing array, pop and verify order)
  - Miri validation of `copy_nonoverlapping` calls (UB detection for the `MaybeUninit` cast)
  - Differential tests: old per-element loop vs new contiguous copy produce identical results for all batch sizes
  - Property-based tests (proptest): random batch sizes, random fill levels, verify FIFO ordering preserved
- **Performance:** A/B benchmarks via `cargo bench --bench spsc` comparing before/after for each phase. Use `target/bench-report-spsc.json` for automated comparison.
- **Regression:** `scripts/check-asm.sh` as codegen regression gate.
- **Platform coverage:** CI runs on both x86_64 (ubuntu-latest) and aarch64 (macos-latest).

## References

- [Snellman: Ring Buffers](https://www.snellman.net/blog/archive/2016-12-13-ring-buffers/) — unmasked indices, power-of-2 masking
- [psy-lob-saw: SPSC Queue Optimization](http://psy-lob-saw.blogspot.com/2013/03/single-producerconsumer-lock-free-queue.html) — step-by-step optimization, cached indices (1.6x improvement)
- [psy-lob-saw: B-Queue Analysis](http://psy-lob-saw.blogspot.com/2013/11/spsc-iv-look-at-bqueue.html) — batch probing, temporal slipping
- [psy-lob-saw: FastFlow Sparse Data](http://psy-lob-saw.blogspot.com/2013/10/spsc-revisited-part-iii-fastflow-sparse.html) — sparse slot layout (skipped)
- [kjellkod: C++ SPSC Queue](https://kjellkod.wordpress.com/2012/11/28/c-debt-paid-in-full-wait-free-lock-free-queue/) — acquire/release memory model
- [mratsim/weave: SPSC Channel Research](https://github.com/mratsim/weave/blob/master/weave/cross_thread_com/channels_spsc.md) — comprehensive survey
- [rigtorp/SPSCQueue](https://github.com/rigtorp/SPSCQueue) — hugepage allocator pattern
- [Agner Fog: Instruction Tables](https://www.agner.org/optimize/instruction_tables.pdf) — DIV costs 14-57 cycles vs AND at 1 cycle
