# SPSC Ring Buffer Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce SPSC ring latency via software prefetch and increase batch throughput via contiguous memcpy, with codegen regression gating.

**Architecture:** Enhance existing `prefetch()` in `compiler_hints.rs` with x86_64 write-exclusive hints and aarch64 inline asm. Integrate into both `RingEngine` and `CopyRingEngine` push/pop hot paths behind a `prefetch` feature flag. Replace per-element batch loops in `CopyRingEngine` with two-region `copy_nonoverlapping`. Verify codegen before/after via `check-asm.sh`.

**Tech Stack:** Rust (no_std), x86_64 intrinsics (`_mm_prefetch`, `_MM_HINT_ET0`), aarch64 inline asm (`PRFM`), `copy_nonoverlapping`, criterion benchmarks.

**Spec:** `docs/superpowers/specs/2026-03-26-spsc-ring-optimization-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `crates/platform/Cargo.toml` | Add `prefetch` feature flag |
| Modify | `crates/platform/src/intrinsics/compiler_hints.rs` | x86_64 write-exclusive prefetch + aarch64 PRFM |
| Modify | `crates/queue/Cargo.toml` | Add `prefetch` feature forwarding |
| Modify | `crates/queue/src/engine.rs` | Prefetch in `try_push`/`try_pop` |
| Modify | `crates/queue/src/copy_ring/engine.rs` | Prefetch in `push`/`pop` + contiguous batch |
| Modify | `crates/queue/examples/asm_shim.rs` | Add copy-ring batch shims |
| Modify | `scripts/check-asm.sh` | Add copy-ring symbols, instruction count gate |
| Test | `crates/platform/src/intrinsics/compiler_hints.rs` (inline tests) | Prefetch smoke tests |
| Test | `crates/queue/src/copy_ring/engine.rs` (inline tests) | Contiguous batch correctness |

---

## Task 1: Codegen Baseline Capture (Phase 0)

**Files:**
- Modify: `scripts/check-asm.sh:26-31`
- Modify: `crates/queue/examples/asm_shim.rs`

- [ ] **Step 1: Add copy-ring batch shims to asm_shim**

Add these shim functions after the existing ones in `crates/queue/examples/asm_shim.rs`:

```rust
#[inline(never)]
pub fn spsc_copy_push_batch_u64(
    ring: &mut SpscRingCopy<u64, 1024>,
    src: &[u64],
) -> usize {
    ring.push_batch(src)
}

#[inline(never)]
pub fn spsc_copy_pop_batch_u64(
    ring: &mut SpscRingCopy<u64, 1024>,
    dst: &mut [u64],
) -> usize {
    ring.pop_batch(dst)
}
```

Add calls to these in `main()`:

```rust
let batch_src = [0u64; 8];
std::hint::black_box(spsc_copy_push_batch_u64(&mut copy_ring, &batch_src));
let mut batch_dst = [0u64; 8];
std::hint::black_box(spsc_copy_pop_batch_u64(&mut copy_ring, &mut batch_dst));
```

- [ ] **Step 2: Add new symbols to check-asm.sh**

In `scripts/check-asm.sh`, add to the `SYMBOLS` array (after existing entries):

```bash
SYMBOLS=(
    "asm_shim::spsc_push_u64"
    "asm_shim::spsc_pop_u64"
    "asm_shim::spsc_push_bytes64"
    "asm_shim::spsc_pop_bytes64"
    "asm_shim::spsc_copy_push_u64"
    "asm_shim::spsc_copy_pop_u64"
    "asm_shim::spsc_copy_push_batch_u64"
    "asm_shim::spsc_copy_pop_batch_u64"
)
```

- [ ] **Step 3: Run baseline capture**

Run: `./scripts/check-asm.sh --baseline`
Expected: Files created in `target/asm/baseline/` for all 8 symbols. Note instruction counts for single-op functions.

- [ ] **Step 4: Verify baseline files exist**

Run: `ls -la target/asm/baseline/*.s`
Expected: 8 `.s` files with non-zero sizes.

- [ ] **Step 5: Commit**

```bash
git add crates/queue/examples/asm_shim.rs scripts/check-asm.sh
git commit -m "chore: add copy-ring batch shims to asm baseline"
```

---

## Task 2: Enhance Prefetch — x86_64 Write-Exclusive + aarch64 PRFM (Phase 1a)

**Files:**
- Modify: `crates/platform/src/intrinsics/compiler_hints.rs:36-59`

- [ ] **Step 1: Write test for write-prefetch behavior**

Add this test at the bottom of the existing `mod tests` in `compiler_hints.rs`:

```rust
#[test]
fn prefetch_write_does_not_crash() {
    let mut value: u64 = 42;
    prefetch(
        &raw const value,
        PrefetchRW::Write,
        PrefetchLocality::High,
    );
    // Verify write after prefetch still works
    value = 99;
    assert_eq!(value, 99);
}

#[test]
fn prefetch_read_does_not_crash_stack_array() {
    let arr = [0u8; 128];
    prefetch(arr.as_ptr(), PrefetchRW::Read, PrefetchLocality::High);
}
```

- [ ] **Step 2: Run tests to verify they pass (with current no-op/read-only impl)**

Run: `cargo test -p mantis-platform -- compiler_hints`
Expected: All tests pass (prefetch is a hint, current impl works for both).

- [ ] **Step 3: Implement x86_64 write-exclusive prefetch**

Replace the x86_64 block in `prefetch()` (lines 37-54 of `compiler_hints.rs`) with:

```rust
#[cfg(target_arch = "x86_64")]
{
    use core::arch::x86_64::{
        _MM_HINT_ET0, _MM_HINT_NTA, _MM_HINT_T0, _MM_HINT_T1, _MM_HINT_T2,
        _mm_prefetch,
    };
    let hint = ptr.cast::<i8>();
    // SAFETY: prefetch is a hint and never faults, even on invalid addresses.
    // The locality must be a compile-time constant for _mm_prefetch.
    unsafe {
        match (rw, locality) {
            // Write prefetch: use ET0 (exclusive) to bring line in Modified
            // state, avoiding the subsequent RFO on the actual store.
            (PrefetchRW::Write, PrefetchLocality::High) => {
                _mm_prefetch(hint, _MM_HINT_ET0);
            }
            // Non-High write localities fall through to read hints — ET0
            // only exists as a single locality level on x86; for other
            // localities we use the read hint as a reasonable fallback.
            (_, PrefetchLocality::NoTemporal) => _mm_prefetch(hint, _MM_HINT_NTA),
            (_, PrefetchLocality::Low) => _mm_prefetch(hint, _MM_HINT_T2),
            (_, PrefetchLocality::Moderate) => _mm_prefetch(hint, _MM_HINT_T1),
            (_, PrefetchLocality::High) => _mm_prefetch(hint, _MM_HINT_T0),
        }
    }
}
```

- [ ] **Step 4: Implement aarch64 prefetch via inline asm**

Replace the `#[cfg(not(target_arch = "x86_64"))]` fallback block (lines 55-58) with:

```rust
#[cfg(target_arch = "aarch64")]
{
    let addr = ptr.cast::<u8>();
    // SAFETY: PRFM is a hint instruction — it never faults and has no
    // side effects beyond cache management. options(nostack, preserves_flags)
    // tells LLVM it doesn't touch the stack or condition flags.
    unsafe {
        match rw {
            PrefetchRW::Read => {
                core::arch::asm!(
                    "prfm pldl1keep, [{ptr}]",
                    ptr = in(reg) addr,
                    options(nostack, preserves_flags),
                );
            }
            PrefetchRW::Write => {
                core::arch::asm!(
                    "prfm pstl1keep, [{ptr}]",
                    ptr = in(reg) addr,
                    options(nostack, preserves_flags),
                );
            }
        }
    }
    let _ = locality; // Locality encoded in instruction mnemonic (L1KEEP)
}
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
{
    let _ = (ptr, rw, locality);
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p mantis-platform -- compiler_hints`
Expected: All tests pass.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p mantis-platform --all-targets --features alloc,std -- -D warnings`
Expected: No warnings.

- [ ] **Step 7: Check formatting**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 8: Commit**

```bash
git add crates/platform/src/intrinsics/compiler_hints.rs
git commit -m "feat(platform): x86_64 write-exclusive prefetch + aarch64 PRFM support"
```

---

## Task 3: Add Prefetch Feature Flag (Phase 1b)

**Files:**
- Modify: `crates/platform/Cargo.toml:11-14`
- Modify: `crates/queue/Cargo.toml:10-15`

- [ ] **Step 1: Add prefetch feature to mantis-platform**

In `crates/platform/Cargo.toml`, add to `[features]`:

```toml
[features]
default = []
std = []
asm = []
nightly = []
prefetch = []
perf-counters = ["dep:perf-event2"]
```

- [ ] **Step 2: Add prefetch feature forwarding to mantis-queue**

In `crates/queue/Cargo.toml`, add to `[features]`:

```toml
[features]
default = []
std = ["alloc", "mantis-platform/std"]
alloc = []
asm = ["mantis-platform/asm"]
nightly = ["mantis-platform/nightly"]
prefetch = ["mantis-platform/prefetch"]
```

- [ ] **Step 3: Verify it compiles with the feature**

Run: `cargo check -p mantis-queue --features prefetch`
Expected: Compiles without error.

- [ ] **Step 4: Commit**

```bash
git add crates/platform/Cargo.toml crates/queue/Cargo.toml
git commit -m "feat: add prefetch feature flag to platform and queue crates"
```

---

## Task 4: Integrate Prefetch into RingEngine (Phase 1c)

**Files:**
- Modify: `crates/queue/src/engine.rs:69-113`

- [ ] **Step 1: Add conditional prefetch import**

At the top of `crates/queue/src/engine.rs`, after the existing imports (line 12), add:

```rust
#[cfg(feature = "prefetch")]
use mantis_platform::{PrefetchLocality, PrefetchRW, prefetch};
```

- [ ] **Step 2: Add prefetch to try_push**

> **One-ahead prefetch pattern (LMAX Disruptor style):** Push prefetches `next_head`, not `head`.
> The slot at `head` is written in *this* call — by the time prefetch fires, the write is imminent
> and the prefetch has no time to hide latency. Prefetching `next_head` brings the *next call's*
> target slot into cache, overlapping the memory fetch with the current call's work.

In `try_push()`, after `let next_head = I::wrap(head + 1, self.storage.capacity());` (line 77) and before the `if next_head == self.tail_cached.get()` check (line 79), insert:

```rust
// One-ahead prefetch: bring next call's target slot into cache while
// this call does its work. Fires before the capacity check so the
// memory subsystem has maximum time to fetch the line.
#[cfg(feature = "prefetch")]
{
    // SAFETY: next_head < capacity (guaranteed by IndexStrategy::wrap).
    // slot_ptr returns a valid pointer. prefetch is a no-op hint.
    let slot = unsafe { self.storage.slot_ptr(next_head) };
    prefetch(slot.cast::<u8>(), PrefetchRW::Write, PrefetchLocality::High);
}
```

- [ ] **Step 3: Add prefetch to try_pop**

In `try_pop()`, after `let tail = self.tail.load(Ordering::Relaxed);` (line 97) and before the `if tail == self.head_cached.get()` check (line 99), insert:

```rust
// Prefetch the slot we're about to read — fires early to overlap
// with the cache-miss check.
#[cfg(feature = "prefetch")]
{
    // SAFETY: tail < capacity (engine invariant). prefetch is a hint.
    let slot = unsafe { self.storage.slot_ptr(tail) };
    prefetch(slot.cast::<u8>(), PrefetchRW::Read, PrefetchLocality::High);
}
```

- [ ] **Step 4: Run tests without prefetch feature**

Run: `cargo test -p mantis-queue --features alloc,std`
Expected: All existing tests pass (prefetch code not compiled).

- [ ] **Step 5: Run tests with prefetch feature**

Run: `cargo test -p mantis-queue --features alloc,std,prefetch`
Expected: All tests pass. Prefetch is a hint — same behavior.

- [ ] **Step 6: Run Miri (prefetch is no-op under Miri)**

Run: `cargo +nightly miri test -p mantis-queue`
Expected: All tests pass. Miri ignores prefetch hints.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy -p mantis-queue --all-targets --features alloc,std,prefetch -- -D warnings`
Expected: No warnings.

- [ ] **Step 8: Check formatting**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 9: Commit**

```bash
git add crates/queue/src/engine.rs
git commit -m "feat(queue): integrate prefetch into RingEngine push/pop hot paths"
```

---

## Task 5: Integrate Prefetch into CopyRingEngine (Phase 1d)

**Files:**
- Modify: `crates/queue/src/copy_ring/engine.rs:62-101`

- [ ] **Step 1: Add conditional prefetch import**

At the top of `crates/queue/src/copy_ring/engine.rs`, after the existing imports (line 13), add:

```rust
#[cfg(feature = "prefetch")]
use mantis_platform::{PrefetchLocality, PrefetchRW, prefetch};
```

- [ ] **Step 2: Add prefetch to push**

> Same one-ahead prefetch pattern as RingEngine (see Task 4 Step 2).

In `push()`, after `let next_head = I::wrap(head + 1, self.storage.capacity());` (line 64) and before the `if next_head == self.tail_cached.get()` check (line 66), insert:

```rust
// One-ahead prefetch: next call's target slot (see Task 4 design note).
#[cfg(feature = "prefetch")]
{
    // SAFETY: next_head < capacity (guaranteed by IndexStrategy::wrap).
    // slot_ptr returns a valid pointer. prefetch is a no-op hint.
    let slot = unsafe { self.storage.slot_ptr(next_head) };
    prefetch(slot.cast::<u8>(), PrefetchRW::Write, PrefetchLocality::High);
}
```

- [ ] **Step 3: Add prefetch to pop**

In `pop()`, after `let tail = self.tail.load(Ordering::Relaxed);` (line 84) and before the `if tail == self.head_cached.get()` check (line 86), insert:

```rust
#[cfg(feature = "prefetch")]
{
    // SAFETY: tail < capacity (engine invariant). prefetch is a hint.
    let slot = unsafe { self.storage.slot_ptr(tail) };
    prefetch(slot.cast::<u8>(), PrefetchRW::Read, PrefetchLocality::High);
}
```

- [ ] **Step 4: Run tests with prefetch**

Run: `cargo test -p mantis-queue --features alloc,std,prefetch`
Expected: All tests pass.

- [ ] **Step 5: Check formatting**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 6: Commit**

```bash
git add crates/queue/src/copy_ring/engine.rs
git commit -m "feat(queue): integrate prefetch into CopyRingEngine push/pop hot paths"
```

---

## Task 6: Contiguous Batch push_batch (Phase 2a)

**Files:**
- Modify: `crates/queue/src/copy_ring/engine.rs:103-144`

- [ ] **Step 1: Write a targeted wrap-around batch test**

Add this test to the existing `mod tests` block in `crates/queue/src/copy_ring/engine.rs`:

```rust
#[test]
fn push_batch_wraparound_contiguous() {
    // Advance head to near end of buffer, then batch-push across wrap
    let engine = new_engine(); // capacity=8, usable=7
    // Fill 6, drain 6 — head and tail now at index 6
    let fill: Vec<u64> = (0..6).collect();
    engine.push_batch(&fill);
    let mut drain = vec![0u64; 6];
    engine.pop_batch(&mut drain);

    // Now push 5 elements starting at index 6: wraps at index 8 -> 0
    let wrap_src: Vec<u64> = (200..205).collect();
    let pushed = engine.push_batch(&wrap_src);
    assert_eq!(pushed, 5);

    let mut out = vec![0u64; 5];
    let popped = engine.pop_batch(&mut out);
    assert_eq!(popped, 5);
    assert_eq!(out, vec![200, 201, 202, 203, 204]);
}
```

- [ ] **Step 2: Run test to verify it passes (existing per-element impl)**

Run: `cargo test -p mantis-queue --features alloc,std -- copy_ring::engine::tests::push_batch_wraparound_contiguous`
Expected: PASS (existing impl already handles wrap correctly).

- [ ] **Step 3: Replace push_batch with contiguous copy**

Replace the `push_batch` method body (lines 104-144 in `copy_ring/engine.rs`) with:

> **Design note:** This intentionally bypasses `CopyPolicy` SIMD dispatch in favor of
> `copy_nonoverlapping` (compiles to `memcpy`). For bulk copies, `memcpy` auto-vectorizes
> and is faster than per-element SIMD dispatch. The `CopyPolicy` path remains for single-element
> `push`/`pop` where its compile-time kernel selection shines.

```rust
    #[inline]
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

        if free < src.len() {
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

        // Head is always stored as a wrapped index < cap (via I::wrap).
        // first_chunk: slots from head to end of backing array.
        // second_chunk: remaining slots wrapping around from index 0.
        let n = src.len().min(free);
        let first_chunk = n.min(cap - head);
        let second_chunk = n - first_chunk;

        // SAFETY: Storage slots are contiguous in memory (array/slice layout).
        // slot_ptr(head) through slot_ptr(head + first_chunk - 1) are adjacent.
        // MaybeUninit<T> has the same layout as T. SPSC protocol guarantees
        // exclusive producer access to these slots.
        unsafe {
            let dst = self.storage.slot_ptr(head).cast::<T>();
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst, first_chunk);
        }

        if second_chunk > 0 {
            // SAFETY: Same invariants. Wrap-around starts at slot 0.
            unsafe {
                let dst = self.storage.slot_ptr(0).cast::<T>();
                core::ptr::copy_nonoverlapping(
                    src[first_chunk..].as_ptr(),
                    dst,
                    second_chunk,
                );
            }
        }

        self.head.store(I::wrap(head + n, cap), Ordering::Release);
        n
    }
```

- [ ] **Step 4: Run ALL batch tests**

Run: `cargo test -p mantis-queue --features alloc,std -- copy_ring::engine::tests::batch`
Expected: All batch tests pass (push_batch_full_capacity, pop_batch_all, push_batch_partial, pop_batch_partial, batch_empty_slice, batch_wraparound, batch_fifo_ordering, push_batch_wraparound_contiguous).

- [ ] **Step 5: Run Miri to check for UB in copy_nonoverlapping**

Run: `cargo +nightly miri test -p mantis-queue`
Expected: No UB detected. Miri validates pointer casts and copy operations.

- [ ] **Step 6: Check formatting**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 7: Commit**

```bash
git add crates/queue/src/copy_ring/engine.rs
git commit -m "perf(queue): contiguous batch push_batch via copy_nonoverlapping"
```

---

## Task 7: Contiguous Batch pop_batch (Phase 2b)

**Files:**
- Modify: `crates/queue/src/copy_ring/engine.rs:147-184`

- [ ] **Step 1: Write pop_batch wrap-around test**

Add to `mod tests`:

```rust
#[test]
fn pop_batch_wraparound_contiguous() {
    let engine = new_engine(); // capacity=8, usable=7
    // Advance tail to near end
    let fill: Vec<u64> = (0..6).collect();
    engine.push_batch(&fill);
    let mut drain = vec![0u64; 6];
    engine.pop_batch(&mut drain);

    // Push 5 (wraps around), then batch-pop all 5
    let wrap_src: Vec<u64> = (300..305).collect();
    engine.push_batch(&wrap_src);

    let mut out = vec![0u64; 5];
    let popped = engine.pop_batch(&mut out);
    assert_eq!(popped, 5);
    assert_eq!(out, vec![300, 301, 302, 303, 304]);
}
```

- [ ] **Step 2: Run test (passes with existing per-element impl)**

Run: `cargo test -p mantis-queue --features alloc,std -- copy_ring::engine::tests::pop_batch_wraparound_contiguous`
Expected: PASS.

- [ ] **Step 3: Replace pop_batch with contiguous copy**

> **Design note:** Same as `push_batch` — intentionally bypasses `CopyPolicy` in favor of
> `copy_nonoverlapping`/`memcpy` which auto-vectorizes for bulk copies.

Replace the `pop_batch` method body with:

```rust
    #[inline]
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

        if avail < dst.len() {
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
        let first_chunk = n.min(cap - tail);
        let second_chunk = n - first_chunk;

        // SAFETY: Storage slots are contiguous. slot_ptr(tail) through
        // slot_ptr(tail + first_chunk - 1) are adjacent. Slots are initialized
        // (producer wrote them). SPSC protocol guarantees exclusive consumer
        // access. MaybeUninit<T> has the same layout as T.
        unsafe {
            let src = self.storage.slot_ptr(tail).cast::<T>();
            core::ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), first_chunk);
        }

        if second_chunk > 0 {
            // SAFETY: Same invariants. Wrap-around reads from slot 0.
            unsafe {
                let src = self.storage.slot_ptr(0).cast::<T>();
                core::ptr::copy_nonoverlapping(
                    src,
                    dst[first_chunk..].as_mut_ptr(),
                    second_chunk,
                );
            }
        }

        self.tail.store(I::wrap(tail + n, cap), Ordering::Release);
        n
    }
```

- [ ] **Step 4: Run ALL tests + Miri**

Run: `cargo test -p mantis-queue --features alloc,std && cargo +nightly miri test -p mantis-queue`
Expected: All pass, no UB.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p mantis-queue --all-targets --features alloc,std,prefetch -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Check formatting**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 7: Commit**

```bash
git add crates/queue/src/copy_ring/engine.rs
git commit -m "perf(queue): contiguous batch pop_batch via copy_nonoverlapping"
```

---

## Task 8: Differential and Property-Based Tests (Phase 2c)

**Files:**
- Modify: `crates/queue/src/copy_ring/engine.rs` (test module)
- New dev-dependency: `proptest` in `crates/queue/Cargo.toml`

- [ ] **Step 1: Add proptest dev-dependency**

In `crates/queue/Cargo.toml`, add to `[dev-dependencies]`:

```toml
proptest = "1.11.0"
```

- [ ] **Step 2: Write differential test — old loop vs new contiguous batch**

Add to `mod tests` in `crates/queue/src/copy_ring/engine.rs`:

```rust
#[test]
fn push_pop_batch_differential() {
    // Verify contiguous batch produces same results as sequential push/pop
    // for various batch sizes and fill levels.
    for fill_first in 0..7 {
        for batch_size in 1..=7 {
            let engine = new_engine(); // capacity=8, usable=7

            // Advance head/tail by fill_first positions
            for i in 0..fill_first {
                assert!(engine.push(&(i as u64)));
            }
            let mut drain = vec![0u64; fill_first];
            engine.pop_batch(&mut drain);

            // Batch push
            let src: Vec<u64> = (100..100 + batch_size as u64).collect();
            let pushed = engine.push_batch(&src);

            // Batch pop
            let mut dst = vec![0u64; pushed];
            let popped = engine.pop_batch(&mut dst);

            assert_eq!(popped, pushed, "fill={fill_first} batch={batch_size}");
            assert_eq!(
                dst,
                src[..pushed].to_vec(),
                "FIFO violated: fill={fill_first} batch={batch_size}"
            );
        }
    }
}
```

- [ ] **Step 3: Write property-based test with proptest**

Add to `mod tests`:

```rust
#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn batch_fifo_preserved(
            fill_level in 0usize..7,
            batch_size in 1usize..8,
        ) {
            let engine = new_engine();

            // Advance to fill_level
            for i in 0..fill_level {
                engine.push(&(i as u64));
            }
            let mut drain = vec![0u64; fill_level];
            engine.pop_batch(&mut drain);

            let src: Vec<u64> = (0..batch_size as u64).collect();
            let pushed = engine.push_batch(&src);

            let mut dst = vec![0u64; pushed];
            let popped = engine.pop_batch(&mut dst);

            prop_assert_eq!(popped, pushed);
            prop_assert_eq!(dst, src[..pushed].to_vec());
        }
    }
}
```

- [ ] **Step 4: Run all tests including proptest**

Run: `cargo test -p mantis-queue --features alloc,std`
Expected: All pass (including proptest which runs 256 cases by default).

- [ ] **Step 5: Run Miri (proptest won't run under Miri, but differential test will)**

Run: `cargo +nightly miri test -p mantis-queue`
Expected: All non-proptest tests pass, no UB detected.

- [ ] **Step 6: Check formatting**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 7: Commit**

```bash
git add crates/queue/Cargo.toml crates/queue/src/copy_ring/engine.rs
git commit -m "test(queue): differential and property-based tests for contiguous batch ops"
```

---

## Task 9: Codegen Verification (Phase 3)

**Files:**
- Modify: `scripts/check-asm.sh`

- [ ] **Step 1: Run codegen diff against baseline**

Run: `./scripts/check-asm.sh`
Expected: Output shows diffs. Single-op functions should show +1-2 instructions (prefetch hint). Batch functions should show reduced instruction count (no per-element loop).

- [ ] **Step 2: Verify prefetch instruction present in push asm**

Run: `./scripts/check-asm.sh --symbol "asm_shim::spsc_push_u64" | grep -i "prefetch\|prfm\|pld"`

On x86_64, expected: `prefetcht0` or `prefetchw` instruction.
On aarch64, expected: `prfm` instruction.

If the `prefetch` feature isn't enabled in the asm_shim build, enable it:
The asm_shim is an example in mantis-queue, so run: `cargo asm -p mantis-queue --example asm_shim --features prefetch "asm_shim::spsc_push_u64"`

- [ ] **Step 3: Check hot-path instruction count delta**

Compare line counts in `target/asm/baseline/*.s` vs `target/asm/*.s` for single-op functions.

Success criteria from spec:
- Hot-path instruction count increase <= 2
- No new branches in fast path
- Cold path remains out-of-line

- [ ] **Step 4: Add instruction count check to check-asm.sh**

Add this block at the end of `scripts/check-asm.sh`, before the final "Done" output:

```bash
# Instruction count soft gate (single-op functions only)
SINGLE_OPS=(
    "spsc_push_u64"
    "spsc_pop_u64"
    "spsc_copy_push_u64"
    "spsc_copy_pop_u64"
)

if [[ -d "$BASELINE_DIR" && "$OUTPUT_DIR" != "$BASELINE_DIR" ]]; then
    echo ""
    echo "=== Instruction count gate ==="
    gate_failed=0
    for sym in "${SINGLE_OPS[@]}"; do
        base_file="$BASELINE_DIR/${sym}.s"
        curr_file="$OUTPUT_DIR/${sym}.s"
        if [[ -f "$base_file" && -f "$curr_file" ]]; then
            base_count=$(grep -cE '^\s+[a-z]' "$base_file" || echo 0)
            curr_count=$(grep -cE '^\s+[a-z]' "$curr_file" || echo 0)
            delta=$((curr_count - base_count))
            if [[ $delta -gt 2 ]]; then
                echo "GATE FAIL: $sym grew by $delta instructions ($base_count -> $curr_count)"
                gate_failed=1
            else
                echo "OK: $sym delta=$delta ($base_count -> $curr_count)"
            fi
        fi
    done
    if [[ $gate_failed -eq 1 ]]; then
        echo "WARNING: Instruction count gate failed for one or more functions."
    fi
fi
```

- [ ] **Step 5: Commit**

```bash
git add scripts/check-asm.sh
git commit -m "chore: add instruction count regression gate to check-asm.sh"
```

---

## Task 10: Benchmark and Validate (Phase 3 continued)

**Files:**
- No file changes — benchmark runs only.

- [ ] **Step 1: Run benchmarks without prefetch**

Run: `cargo bench --bench spsc`
Expected: Baseline results captured in `target/criterion/`.

- [ ] **Step 2: Run benchmarks with prefetch**

Run: `cargo bench --bench spsc --features prefetch`
Expected: Results. Compare single-op and batch results against non-prefetch baseline in criterion HTML report.

- [ ] **Step 3: Run benchmarks with native CPU flags**

Run: `RUSTFLAGS='-C target-cpu=native' cargo bench --bench spsc --features prefetch`
Expected: Best-case results with native instruction selection.

- [ ] **Step 4: Evaluate rollback criteria**

Check criterion output for:
- Single-op (u64): Should improve or stay flat (1-3ns improvement on cache-cold)
- Burst/batch (100, 1000): Should show 20-50% improvement from contiguous copy
- If any workload regresses >2% on aarch64, the prefetch feature should be disabled for that platform

- [ ] **Step 5: Final full test suite**

Run: `cargo test --features alloc,std,prefetch && cargo +nightly miri test -p mantis-queue && cargo clippy --all-targets --features alloc,std,prefetch -- -D warnings && cargo fmt --all --check`
Expected: All green.

- [ ] **Step 6: Commit any final adjustments, update PROGRESS.md**

```bash
git add docs/PROGRESS.md
git commit -m "docs: mark Phase 1-3 SPSC optimizations complete"
```
