# mantis-platform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `mantis-platform` crate with full Constantine `platforms/` parity: constant-time types, extended precision arithmetic, compiler hints, CPUID detection, macro assembler types, SIMD copy kernels, cycle counters, and cache padding.

**Architecture:** New leaf crate at `crates/platform/` with `no_std` default. Constant-time types (`Ct<T>`, `CTBool<T>`, `Carry`, `Borrow`) as foundation. ISA-specific code in `isa_x86/` and `isa_arm64/` modules. Portable code at root and `intrinsics/`. Existing code migrates from queue/bench with preserved APIs.

**Tech Stack:** Rust (no_std + std features), cfg-if, libc (Linux ARM64), inline asm (x86_64 RDTSC + cmov)

**Spec:** `docs/specs/2026-03-26-mantis-platform-crate-design.md`

---

## File Map

### New files (crates/platform/)

| File | Responsibility |
|---|---|
| `Cargo.toml` | Crate manifest with features: std, asm, nightly |
| `README.md` | Crate documentation |
| `src/lib.rs` | `#![no_std]`, feature gates, top-level re-exports |
| `src/config.rs` | Compile-time platform constants |
| `src/bithacks.rs` | Vartime bit operations |
| `src/pad.rs` | `CachePadded<T>` (moved from queue) |
| `src/constant_time/mod.rs` | Re-exports for constant-time submodule |
| `src/constant_time/ct_types.rs` | `Ct<T>`, `CTBool<T>`, `Carry`, `Borrow`, `VarTime` |
| `src/constant_time/ct_routines.rs` | Bitwise ops, comparisons, cneg, isZero, isNonZero |
| `src/constant_time/multiplexers.rs` | `mux`, `ccopy`, `secret_lookup` (portable + x86 cmov) |
| `src/constant_time/ct_division.rs` | `div2n1n`: constant-time division |
| `src/intrinsics/mod.rs` | Re-exports for intrinsics submodule |
| `src/intrinsics/copy_policy.rs` | `CopyPolicy` trait (moved from core) |
| `src/intrinsics/copy_dispatch.rs` | `CopyDispatcher`, `DefaultCopyPolicy`, `SimdCopyPolicy` (moved from queue) |
| `src/intrinsics/addcarry_subborrow.rs` | `AddCarryOp`, `SubBorrowOp` traits with `Carry`/`Borrow` |
| `src/intrinsics/extended_prec.rs` | `WideMul`, `WideMulAdd1`, `WideMulAdd2`, `SignedWideMul`, `mul_acc`, `mul_double_acc` |
| `src/intrinsics/compiler_hints.rs` | `prefetch`, `prefetch_large`, `PrefetchRW`, `PrefetchLocality` |
| `src/isa_x86/mod.rs` | x86_64 re-exports |
| `src/isa_x86/simd.rs` | Placeholder (SSE2 kernels in copy_dispatch.rs for now) |
| `src/isa_x86/rdtsc.rs` | `RdtscCounter` (moved from bench) |
| `src/isa_x86/cpudetect.rs` | CPUID feature detection with load-time caching |
| `src/isa_x86/assembler.rs` | RM, Constraint, MemIndirectAccess, Register, Operand, AssemblerX86 |
| `src/isa_arm64/mod.rs` | aarch64 re-exports |
| `src/isa_arm64/simd.rs` | Placeholder (NEON kernels in copy_dispatch.rs for now) |
| `src/isa_arm64/counters.rs` | `KperfCounter` + `PmuCounter` (moved from bench) |
| `src/isa_arm64/assembler.rs` | RM, ConditionCode, Constraint, MemIndirectAccess, Register, Operand |
| `src/metering/mod.rs` | `CycleCounter` trait, `Measurement`, `DefaultCounter` |
| `src/metering/instant.rs` | `InstantCounter` (moved from bench) |
| `src/cpudetect.rs` | `cpu_name()` (extracted from bench/report.rs) |

### Modified files

| File | Changes |
|---|---|
| `Cargo.toml` (workspace) | Add `mantis-platform`, `cfg-if` to workspace deps |
| `crates/core/src/lib.rs` | Remove `CopyPolicy` trait |
| `crates/queue/Cargo.toml` | Add `mantis-platform` dependency |
| `crates/queue/src/lib.rs` | Re-export `CachePadded` from platform, remove `mod pad` |
| `crates/queue/src/engine.rs` | Import `CachePadded` from platform |
| `crates/queue/src/copy_ring/engine.rs` | Import `CopyPolicy` from platform |
| `crates/queue/src/copy_ring/mod.rs` | Import `CopyPolicy` from platform |
| `crates/queue/src/copy_ring/handle.rs` | Import `CopyPolicy`, `DefaultCopyPolicy` from platform |
| `crates/queue/src/copy_ring/raw/mod.rs` | Import `CopyPolicy`, re-export from platform |
| `crates/queue/src/presets.rs` | Import `DefaultCopyPolicy` from platform |
| `crates/bench/Cargo.toml` | Add `mantis-platform` dep, forward `asm` feature |
| `crates/bench/src/lib.rs` | Remove `pub mod counters` |
| `crates/bench/src/measurement.rs` | Import from `mantis_platform::metering` |
| `crates/bench/src/report.rs` | Use `mantis_platform::cpudetect::cpu_name()` |
| `README.md` (workspace root) | Add mantis-platform to crate listing |
| `CLAUDE.md` | Add platform to workspace layout |
| `docs/PROGRESS.md` | Add mantis-platform to crate status |

### Deleted files

| File | Reason |
|---|---|
| `crates/queue/src/pad.rs` | Moved to platform |
| `crates/queue/src/copy_ring/raw/simd.rs` | Moved to platform/intrinsics/copy_dispatch.rs |
| `crates/bench/src/counters/mod.rs` | Moved to platform/metering |
| `crates/bench/src/counters/rdtsc.rs` | Moved to platform/isa_x86 |
| `crates/bench/src/counters/kperf.rs` | Moved to platform/isa_arm64/counters.rs |
| `crates/bench/src/counters/pmu.rs` | Moved to platform/isa_arm64/counters.rs |
| `crates/bench/src/counters/instant.rs` | Moved to platform/metering |

---

## Task 1: Scaffold crate with Cargo.toml and lib.rs

**Files:**
- Create: `crates/platform/Cargo.toml`
- Create: `crates/platform/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Add `cfg-if` and `mantis-platform` to workspace dependencies**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:

```toml
mantis-platform = { path = "crates/platform", version = "0.1.0" }
cfg-if = "1"
```

And add `"crates/platform"` to `[workspace] members`.

- [ ] **Step 2: Create `crates/platform/Cargo.toml`**

```toml
[package]
name = "mantis-platform"
description = "Platform abstractions, constant-time types, SIMD intrinsics, and cycle counters for the Mantis SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
std = []
asm = []
nightly = []

[dependencies]
cfg-if = { workspace = true }

[target.'cfg(all(target_arch = "aarch64", target_os = "linux"))'.dependencies]
libc = { workspace = true, default-features = false }

[lints]
workspace = true
```

- [ ] **Step 3: Create `crates/platform/src/lib.rs`**

```rust
//! Platform abstractions for the Mantis SDK.
//!
//! Consolidates all platform-specific code: constant-time types, compile-time
//! ISA detection, SIMD copy kernels, cycle counters, cache-line padding,
//! extended precision arithmetic, and bit operations. Full parity with
//! Constantine's `platforms/` module.
//!
//! This crate is `no_std` by default. Enable `std` for `InstantCounter`,
//! CPUID detection, and CPU name. Enable `asm` for `RdtscCounter` on x86_64.

#![no_std]
#![deny(unsafe_code)]
#![cfg_attr(feature = "nightly", feature(generic_const_exprs))]
#![cfg_attr(feature = "nightly", allow(incomplete_features))]

#[cfg(feature = "std")]
extern crate std;

pub mod config;
pub mod bithacks;
pub mod pad;
pub mod constant_time;
pub mod intrinsics;
pub mod metering;

#[cfg(target_arch = "x86_64")]
pub mod isa_x86;

#[cfg(target_arch = "aarch64")]
pub mod isa_arm64;

#[cfg(feature = "std")]
pub mod cpudetect;

// Top-level re-exports for convenience
pub use pad::CachePadded;
pub use intrinsics::CopyPolicy;
pub use intrinsics::DefaultCopyPolicy;
pub use constant_time::{Ct, CTBool, Carry, Borrow};
#[cfg(feature = "nightly")]
pub use intrinsics::SimdCopyPolicy;
```

- [ ] **Step 4: Verify Cargo.toml is valid**

Run: `cargo metadata --format-version 1 | jq '.packages[] | select(.name == "mantis-platform") | .name'`

Expected: `"mantis-platform"`

- [ ] **Step 5: Commit**

```
feat(platform): scaffold mantis-platform crate
```

---

## Task 2: config.rs and bithacks.rs

**Files:**
- Create: `crates/platform/src/config.rs`
- Create: `crates/platform/src/bithacks.rs`

- [ ] **Step 1: Create `crates/platform/src/config.rs`**

```rust
//! Compile-time platform detection constants.

/// `true` when compiling for x86_64.
pub const X86_64: bool = cfg!(target_arch = "x86_64");

/// `true` when compiling for aarch64.
pub const AARCH64: bool = cfg!(target_arch = "aarch64");

/// `true` when targeting macOS.
pub const IS_MACOS: bool = cfg!(target_os = "macos");

/// `true` when targeting Linux.
pub const IS_LINUX: bool = cfg!(target_os = "linux");

/// Conservative cache-line size in bytes.
///
/// 128 covers both Intel (64B) and Apple Silicon (128B).
pub const CACHE_LINE: usize = 128;
```

- [ ] **Step 2: Create `crates/platform/src/bithacks.rs` with tests**

Contains: `is_power_of_two`, `next_power_of_two`, `log2_floor`, `trailing_zeros`, `round_up`, `ceil_div_vartime`. All `const fn`. Full test suite with edge cases. Follows existing plan code exactly, plus add:

```rust
/// Ceiling division: `ceil(a / b)`.
///
/// # Panics
///
/// Panics if `b` is zero.
#[must_use]
pub const fn ceil_div_vartime(a: usize, b: usize) -> usize {
    assert!(b > 0, "division by zero");
    (a + b - 1) / b
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- bithacks`

- [ ] **Step 4: Commit**

```
feat(platform): add config.rs and bithacks.rs
```

---

## Task 3: pad.rs (move from queue)

**Files:**
- Create: `crates/platform/src/pad.rs`

- [ ] **Step 1: Copy exact content from `crates/queue/src/pad.rs`**

No changes needed — no crate-internal dependencies.

- [ ] **Step 2: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- pad`

- [ ] **Step 3: Commit**

```
feat(platform): add CachePadded (moved from mantis-queue)
```

---

## Task 4: constant_time/ct_types.rs

**Files:**
- Create: `crates/platform/src/constant_time/mod.rs`
- Create: `crates/platform/src/constant_time/ct_types.rs`

- [ ] **Step 1: Create `crates/platform/src/constant_time/mod.rs`**

```rust
//! Constant-time types and operations.
//!
//! Prevents the compiler from optimizing bitwise operations into
//! conditional branches, protecting against timing side-channels.
//! Maps from Constantine's `constant_time/` module.

pub mod ct_types;
pub mod ct_routines;
pub mod multiplexers;
pub mod ct_division;

pub use ct_types::{Ct, CTBool, Carry, Borrow, VarTime};
pub use ct_routines::*;
pub use multiplexers::{mux, ccopy, secret_lookup};
pub use ct_division::div2n1n;
```

- [ ] **Step 2: Create `crates/platform/src/constant_time/ct_types.rs`**

```rust
//! Fundamental constant-time types.
//!
//! `Ct<T>` wraps unsigned integers to prevent the compiler from
//! replacing bitwise operations with branches. `CTBool<T>` restricts
//! to 0/1 range for constant-time boolean logic.

use core::marker::PhantomData;

/// Constant-time unsigned integer wrapper.
///
/// All operations on `Ct<T>` are implemented via bitwise manipulation,
/// preventing the compiler from introducing conditional branches.
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
pub struct Ct<T>(pub(crate) T);

/// Constant-time boolean, restricted to 0 or 1.
///
/// Generic parameter `T` is the inner unsigned type (e.g., `u64`).
/// The inner field is `Ct<T>`, so `CTBool<u64>` contains `Ct<u64>`.
/// We don't use `bool` because the compiler can and will optimize
/// boolean operations into branches.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CTBool<T>(pub(crate) Ct<T>);

/// Carry flag for addition chains. Alias for `Ct<u8>`.
pub type Carry = Ct<u8>;

/// Borrow flag for subtraction chains. Alias for `Ct<u8>`.
pub type Borrow = Ct<u8>;

/// Marker type for variable-time operations (effect tracking).
///
/// Functions tagged with `VarTime` in their signature are explicitly
/// variable-time and must not be called from constant-time contexts.
pub struct VarTime;

impl<T> Ct<T> {
    /// Wrap a value in the constant-time type.
    #[inline]
    pub const fn new(val: T) -> Self {
        Self(val)
    }

    /// Unwrap the inner value.
    ///
    /// Use sparingly — escaping `Ct<T>` loses timing guarantees.
    #[inline]
    pub const fn inner(self) -> T where T: Copy {
        self.0
    }
}

impl CTBool<u64> {
    /// Constant-time `true`.
    #[inline]
    pub fn ctrue() -> Self {
        CTBool(Ct(1))
    }

    /// Constant-time `false`.
    #[inline]
    pub fn cfalse() -> Self {
        CTBool(Ct(0))
    }
}

// Repeat for u32, u8, usize via macro_rules!

// Debug impls that don't leak values
impl<T> core::fmt::Debug for Ct<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Ct(***)")
    }
}

impl<T> core::fmt::Debug for CTBool<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("CTBool(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_construction() {
        let x = Ct::new(42u64);
        assert_eq!(x.inner(), 42);
    }

    #[test]
    fn carry_borrow_are_ct_u8() {
        let c: Carry = Ct::new(1u8);
        let b: Borrow = Ct::new(0u8);
        assert_eq!(c.inner(), 1);
        assert_eq!(b.inner(), 0);
    }

    #[test]
    fn vartime_is_zst() {
        assert_eq!(core::mem::size_of::<VarTime>(), 0);
    }

    #[test]
    fn ct_debug_does_not_leak() {
        let x = Ct::new(42u64);
        let s = format!("{x:?}");
        assert!(!s.contains("42"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- ct_types`

- [ ] **Step 4: Commit**

```
feat(platform): add constant-time types (Ct<T>, CTBool<T>, Carry, Borrow)
```

---

## Task 5: constant_time/ct_routines.rs

**Files:**
- Create: `crates/platform/src/constant_time/ct_routines.rs`

- [ ] **Step 1: Create ct_routines.rs**

Implement all constant-time arithmetic and comparison operations on `Ct<T>` using bitwise manipulation only. Maps from Constantine's `ct_routines.nim`.

Operations needed (implemented via macro for u8, u16, u32, u64, usize):
- `and`, `or`, `xor`, `not`, `+`, `-`, `shr`, `shl`, `*`
- Comparisons returning `CTBool<T>`: `ct_eq`, `ct_lt`, `ct_le`, `ct_ne`
- `is_msb_set`, `is_zero`, `is_non_zero`
- `cneg` (conditional negate)

Use a macro to implement for all unsigned integer types. Each operation must be `#[inline]` and use only bitwise/arithmetic ops (no `if`, no `match` on values).

Full test suite comparing against stdlib operations.

- [ ] **Step 2: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- ct_routines`

- [ ] **Step 3: Commit**

```
feat(platform): add constant-time routines (bitwise ops, comparisons)
```

---

## Task 6: constant_time/multiplexers.rs

**Files:**
- Create: `crates/platform/src/constant_time/multiplexers.rs`

- [ ] **Step 1: Create multiplexers.rs**

Three functions mapping from Constantine's `multiplexers.nim`:

```rust
/// Constant-time select: returns x if ctl is true, y otherwise.
/// Portable: y ^ (-T(ctl) & (x ^ y))
/// x86_64: test + cmovz (inline asm, behind cfg(target_arch))
pub fn mux<T>(ctl: CTBool<T>, x: Ct<T>, y: Ct<T>) -> Ct<T>;

/// Conditional copy: x = ctl ? y : x
pub fn ccopy<T>(ctl: CTBool<T>, x: &mut Ct<T>, y: Ct<T>);

/// Constant-time table lookup (scans entire table).
pub fn secret_lookup<T: Copy + Default>(table: &[T], index: Ct<usize>) -> T;
```

x86 cmov implementation: use `core::arch::asm!` with `test + cmovnz` for both u32 and u64, matching Constantine's `mux_x86` and `ccopy_x86`. Gate on `cfg(target_arch = "x86_64")`.

The x86 asm block requires `#![allow(unsafe_code)]` file-level attribute.

Full test suite: all mux combos, ccopy conditional/no-op, secret_lookup correctness + bounds.

- [ ] **Step 2: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- multiplexers`

- [ ] **Step 3: Commit**

```
feat(platform): add constant-time multiplexers (mux, ccopy, secret_lookup)
```

---

## Task 7: constant_time/ct_division.rs

**Files:**
- Create: `crates/platform/src/constant_time/ct_division.rs`

- [ ] **Step 1: Create ct_division.rs**

Implement `div2n1n` mapping from Constantine's `ct_division.nim` (BearSSL algorithm):

```rust
/// Constant-time division of double-width by single-width.
/// (quotient, remainder) <- (n_hi, n_lo) / d
///
/// Preconditions: n_hi < d, d's MSB should be set (normalized).
pub fn div2n1n(n_hi: Ct<u64>, n_lo: Ct<u64>, d: Ct<u64>) -> (Ct<u64>, Ct<u64>);
```

Uses `mux` from multiplexers for constant-time conditional updates. Also implement for `Ct<u32>`.

Full test suite: basic division, remainder, edge cases, cross-check against stdlib div.

- [ ] **Step 2: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- ct_division`

- [ ] **Step 3: Commit**

```
feat(platform): add constant-time division (div2n1n)
```

---

## Task 8: intrinsics/addcarry_subborrow.rs

**Files:**
- Create: `crates/platform/src/intrinsics/addcarry_subborrow.rs`
- Modify: `crates/platform/src/intrinsics/mod.rs`

- [ ] **Step 1: Create addcarry_subborrow.rs**

Maps from Constantine's `addcarry_subborrow.nim`:

```rust
use crate::constant_time::{Ct, Carry, Borrow};

/// Addition with carry.
pub trait AddCarryOp: Sized {
    /// (sum, carry_out) <- a + b + carry_in
    fn add_c(self, rhs: Self, carry_in: Carry) -> (Self, Carry);
}

/// Subtraction with borrow.
pub trait SubBorrowOp: Sized {
    /// (diff, borrow_out) <- a - b - borrow_in
    fn sub_b(self, rhs: Self, borrow_in: Borrow) -> (Self, Borrow);
}
```

Portable fallback for Ct<u32> (via u64 widening) and Ct<u64> (via u128 widening).

x86_64 path: use `_addcarry_u64` / `_subborrow_u64` intrinsics via `core::arch::x86_64` when `cfg(target_arch = "x86_64")`. Note: Rust's `core::arch::x86_64` exposes `_addcarry_u64` since nightly — check availability and fall back to portable if not stable.

Actually, for stable Rust, use the portable widening approach (which LLVM optimizes well on x86). Add ISA-specific asm paths behind `feature = "asm"` when needed for guaranteed codegen.

Full test suite: no carry, carry in, carry out, full carry chain (4-limb add). Same for borrow.

- [ ] **Step 2: Update intrinsics/mod.rs**

Add `pub mod addcarry_subborrow;` and re-exports.

- [ ] **Step 3: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- addcarry`

- [ ] **Step 4: Commit**

```
feat(platform): add addC/subB with Carry/Borrow types
```

---

## Task 9: intrinsics/extended_prec.rs (full)

**Files:**
- Create: `crates/platform/src/intrinsics/extended_prec.rs`

- [ ] **Step 1: Create extended_prec.rs**

Maps from Constantine's `extended_precision.nim`. Full set of operations:

```rust
use crate::constant_time::Ct;
use crate::intrinsics::addcarry_subborrow::AddCarryOp;

/// Widening multiply: (hi, lo) <- a * b
pub trait WideMul: Sized {
    fn wide_mul(self, rhs: Self) -> (Self, Self);
}

/// Widening multiply + single add: (hi, lo) <- a * b + c
pub trait WideMulAdd1: WideMul {
    fn muladd1(self, rhs: Self, c: Self) -> (Self, Self);
}

/// Widening multiply + double add: (hi, lo) <- a * b + c1 + c2
pub trait WideMulAdd2: WideMul {
    fn muladd2(self, rhs: Self, c1: Self, c2: Self) -> (Self, Self);
}

/// Signed widening multiply: (hi, lo) <- a * b (signed interpretation)
pub trait SignedWideMul: Sized {
    fn smul(self, rhs: Self) -> (Self, Self);
}

/// 3-limb accumulate: (t, u, v) += a * b
pub fn mul_acc<T: WideMul + AddCarryOp>(t: &mut T, u: &mut T, v: &mut T, a: T, b: T);

/// 3-limb double accumulate: (t, u, v) += 2 * a * b
pub fn mul_double_acc<T: WideMul + AddCarryOp>(t: &mut T, u: &mut T, v: &mut T, a: T, b: T);
```

Portable impls for `Ct<u32>` (via u64) and `Ct<u64>` (via u128). The `mulAcc`/`mulDoubleAcc` implementations use `addC` from `addcarry_subborrow`, matching Constantine exactly.

Full test suite:
- wide_mul: max * max, 0 * x, identity
- muladd1: verify no-overflow invariant (max² + max fits in double width)
- muladd2: same + double add
- smul: signed correctness, negative × negative, mixed signs
- mul_acc: accumulation correctness
- mul_double_acc: double accumulation correctness

- [ ] **Step 2: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- extended_prec`

- [ ] **Step 3: Commit**

```
feat(platform): add full extended precision arithmetic
```

---

## Task 10: intrinsics/compiler_hints.rs

**Files:**
- Create: `crates/platform/src/intrinsics/compiler_hints.rs`

- [ ] **Step 1: Create compiler_hints.rs**

Maps from Constantine's `compiler_optim_hints.nim` + `primitives.nim` prefetch:

```rust
//! Compiler optimization hints: prefetch, assume_aligned.

/// Prefetch direction.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchRW {
    Read = 0,
    Write = 1,
}

/// Prefetch temporal locality hint.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchLocality {
    /// Data can be discarded from CPU cache after access.
    NoTemporal = 0,
    /// L1 cache eviction level.
    Low = 1,
    /// L2 cache eviction level.
    Moderate = 2,
    /// Data should be left in all levels of cache.
    High = 3,
}

/// Prefetch a cache line containing `ptr`.
///
/// This is a hint — the CPU may ignore it. On platforms without
/// prefetch support, this is a no-op.
#[inline]
pub fn prefetch<T>(ptr: *const T, _rw: PrefetchRW, _locality: PrefetchLocality) {
    // x86_64: _mm_prefetch
    // aarch64: __prefetch
    // other: no-op
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: prefetch is a hint, never causes faults on valid or invalid addresses
        #[allow(unsafe_code)]
        unsafe {
            core::arch::x86_64::_mm_prefetch(ptr.cast::<i8>(), _locality as i32);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = ptr;
    }
}

/// Prefetch a large value spanning multiple cache lines.
///
/// Prefetches up to `max_lines` cache lines (0 = all lines covering T).
#[inline]
pub fn prefetch_large<T>(
    ptr: *const T,
    rw: PrefetchRW,
    locality: PrefetchLocality,
    max_lines: usize,
) {
    let span = core::mem::size_of::<T>() / 64; // 64-byte cache lines
    let n = if max_lines == 0 { span } else { span.min(max_lines) };
    for i in 0..n {
        // SAFETY: pointer arithmetic for prefetch hint; never dereferenced
        #[allow(unsafe_code)]
        let line_ptr = unsafe { (ptr as *const u8).add(i * 64) };
        prefetch(line_ptr, rw, locality);
    }
}
```

- [ ] **Step 2: Update intrinsics/mod.rs**

Add `pub mod compiler_hints;` and re-exports.

- [ ] **Step 3: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- compiler_hints`

Tests: prefetch doesn't crash, prefetch_large handles large types.

- [ ] **Step 4: Commit**

```
feat(platform): add compiler hints (prefetch, prefetch_large)
```

---

## Task 11: intrinsics/ — CopyPolicy and CopyDispatch (move from queue)

**Files:**
- Create: `crates/platform/src/intrinsics/copy_policy.rs`
- Create: `crates/platform/src/intrinsics/copy_dispatch.rs`
- Modify: `crates/platform/src/intrinsics/mod.rs`

- [ ] **Step 1: Create copy_policy.rs**

Move `CopyPolicy` trait from `crates/core/src/lib.rs`. Same code, new location.

- [ ] **Step 2: Create copy_dispatch.rs**

Copy entire content of `crates/queue/src/copy_ring/raw/simd.rs` into this file. Change only:
- `use mantis_core::CopyPolicy` → `use crate::intrinsics::copy_policy::CopyPolicy`
- Keep all `#[cfg(target_arch)]` guards as-is
- Keep all tests as-is
- Add `// FIXME: SSE2/NEON kernels should move to isa_x86/simd.rs and isa_arm64/simd.rs` at top

- [ ] **Step 3: Update intrinsics/mod.rs with all re-exports**

```rust
pub mod copy_policy;
pub mod copy_dispatch;
pub mod addcarry_subborrow;
pub mod extended_prec;
pub mod compiler_hints;

pub use copy_policy::CopyPolicy;
pub use copy_dispatch::DefaultCopyPolicy;
#[cfg(feature = "nightly")]
pub use copy_dispatch::SimdCopyPolicy;
pub use addcarry_subborrow::{AddCarryOp, SubBorrowOp};
pub use extended_prec::{WideMul, WideMulAdd1, WideMulAdd2, SignedWideMul};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- intrinsics`

- [ ] **Step 5: Commit**

```
feat(platform): add CopyPolicy and CopyDispatcher (moved from mantis-queue)
```

---

## Task 12: ISA modules — isa_x86/ assembler and simd

**Files:**
- Create: `crates/platform/src/isa_x86/mod.rs`
- Create: `crates/platform/src/isa_x86/simd.rs`
- Create: `crates/platform/src/isa_x86/assembler.rs`

- [ ] **Step 1: Create isa_x86/mod.rs**

```rust
//! x86_64 platform support.

pub mod simd;
pub mod assembler;

#[cfg(all(feature = "asm", feature = "std"))]
pub mod rdtsc;

#[cfg(feature = "std")]
pub mod cpudetect;
```

- [ ] **Step 2: Create isa_x86/simd.rs**

Placeholder — SSE2 kernels remain in `intrinsics/copy_dispatch.rs` for now.

```rust
//! x86_64 SIMD utilities.
//!
//! SSE2 load/store kernels are currently in `intrinsics/copy_dispatch.rs`
//! alongside the dispatch logic.
//! FIXME: Move SSE2 kernels here when dispatch macros are refactored.
//! Future AVX2/AVX-512 kernels will live here.
```

- [ ] **Step 3: Create isa_x86/assembler.rs**

Full type definitions mapping from Constantine's `macro_assembler_x86_att.nim`:

```rust
//! x86_64 macro assembler types for structured assembly.
//!
//! Type definitions for the compile-time assembler DSL.
//! Maps from Constantine's `macro_assembler_x86_att.nim`.

/// Register or Memory operand kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RM {
    Reg, Mem, Imm, MemOffsettable,
    PointerInReg, ElemsInReg,
    RCX, RDX, R8, RAX,
    CarryFlag, ClobberedReg,
}

/// GCC extended assembly constraint modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    Input, InputCommutative,
    OutputOverwrite, OutputEarlyClobber,
    InputOutput, InputOutputEarlyClobber,
    ClobberedRegister,
}

/// Memory indirect access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemIndirectAccess {
    NoAccess, Read, Write, ReadWrite,
}

/// x86_64 general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    Rbx, Rdx, R8, Rax, Xmm0,
}

/// Operand kind discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Register, FromArray, ArrayAddr, Array2dAddr,
}

/// Assembly operand.
#[derive(Debug, Clone)]
pub enum Operand {
    Reg(Register),
    Imm(i64),
    Mem { base: Register, offset: i32 },
    FromArray { base: Register, offset: usize },
}

// Note: AssemblerX86 struct (code buffer + operand tracking) requires
// String/Vec which need alloc. It will be added when the `alloc` feature
// is introduced. For now, only the zero-allocation enum types are defined —
// they are the core of the DSL and sufficient for type-safe operand
// construction.
```

Full test suite: all enum variants constructable, pattern matching.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-platform --no-default-features -- assembler` (if on x86)

- [ ] **Step 5: Commit**

```
feat(platform): add isa_x86 module with assembler types
```

---

## Task 13: ISA modules — isa_arm64/ assembler and simd

**Files:**
- Create: `crates/platform/src/isa_arm64/mod.rs`
- Create: `crates/platform/src/isa_arm64/simd.rs`
- Create: `crates/platform/src/isa_arm64/assembler.rs`

- [ ] **Step 1: Create isa_arm64/mod.rs**

```rust
//! aarch64 platform support.

pub mod simd;
pub mod assembler;
pub mod counters;
```

- [ ] **Step 2: Create isa_arm64/simd.rs**

Placeholder (same pattern as x86).

- [ ] **Step 3: Create isa_arm64/assembler.rs**

Full type definitions mapping from Constantine's `macro_assembler_arm64.nim`:

```rust
//! aarch64 macro assembler types for structured assembly.

/// Register or Memory operand kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RM {
    Reg, Mem, Imm, MemOffsettable,
    PointerInReg, ElemsInReg,
    XZR,
    /// ARM carry flag (set on no-borrow for subtraction).
    CarryFlag,
    /// ARM borrow flag (inverted carry semantics).
    BorrowFlag,
    ClobberedReg,
}

/// ARM64 condition codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionCode {
    Eq, Ne, Cs, Hs, Cc, Lo, Mi, Pl,
    Vs, Vc, Hi, Ls, Ge, Lt, Gt, Le, Al,
}

/// GCC extended assembly constraint modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    Input, InputCommutative,
    OutputOverwrite, OutputEarlyClobber,
    InputOutput, InputOutputEarlyClobber,
    ClobberedRegister,
}

/// Memory indirect access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemIndirectAccess {
    NoAccess, Read, Write, ReadWrite,
}

/// aarch64 general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register { Xzr }

/// Operand kind discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Register, FromArray, ArrayAddr, Array2dAddr,
}

/// Assembly operand.
#[derive(Debug, Clone)]
pub enum Operand {
    Reg(Register),
    Imm(i64),
    Mem { base: Register, offset: i32 },
}
```

Full test suite: all enum variants, ConditionCode coverage.

- [ ] **Step 4: Run tests (on current platform)**

Run: `cargo test -p mantis-platform --no-default-features`

- [ ] **Step 5: Commit**

```
feat(platform): add isa_arm64 module with assembler types and condition codes
```

---

## Task 14: isa_x86/cpudetect.rs

**Files:**
- Create: `crates/platform/src/isa_x86/cpudetect.rs`

- [ ] **Step 1: Create cpudetect.rs**

Maps from Constantine's `cpudetect_x86.nim`. Key design: CPUID results cached in `OnceLock<CpuFeatures>`, initialized on first access.

```rust
#![allow(unsafe_code)]
//! x86_64 CPUID feature detection with load-time caching.
//!
//! CPUID is ~70 cycles / ~120 latency. Results are cached in a static
//! OnceLock, initialized on first feature query.

use std::sync::OnceLock;

struct CpuIdRegs { eax: u32, ebx: u32, ecx: u32, edx: u32 }

fn cpuid(eax: u32, ecx: u32) -> CpuIdRegs { /* inline asm */ }

pub fn cpu_name_x86() -> String {
    // CPUID leaves 0x80000002-4, same as Constantine
}

struct CpuFeatures {
    has_sse2: bool,
    has_sse3: bool,
    // ... all flags from Constantine
    has_adx: bool,
}

static FEATURES: OnceLock<CpuFeatures> = OnceLock::new();

fn detect() -> &'static CpuFeatures {
    FEATURES.get_or_init(|| {
        // Query CPUID leaf 1 and leaf 7, extract all bits
    })
}

pub fn has_sse2() -> bool { detect().has_sse2 }
pub fn has_avx2() -> bool { detect().has_avx2 }
pub fn has_adx() -> bool { detect().has_adx }
// ... all accessors
```

Full test suite: cpu_name returns non-empty, has_sse2 is true on all x86_64, feature flags don't panic.

- [ ] **Step 2: Run tests (if on x86_64)**

Run: `cargo test -p mantis-platform --features std -- cpudetect`

- [ ] **Step 3: Commit**

```
feat(platform): add x86_64 CPUID feature detection with cached flags
```

---

## Task 15: isa_x86/rdtsc.rs (move from bench)

**Files:**
- Create: `crates/platform/src/isa_x86/rdtsc.rs`

- [ ] **Step 1: Move from bench, update imports**

Copy `crates/bench/src/counters/rdtsc.rs`. Change:
- `use crate::counters::{CycleCounter, Measurement}` → `use crate::metering::{CycleCounter, Measurement}`

- [ ] **Step 2: Run tests (if on x86_64)**

Run: `cargo test -p mantis-platform --features std,asm -- rdtsc`

- [ ] **Step 3: Commit**

```
feat(platform): add RdtscCounter (moved from mantis-bench)
```

---

## Task 16: isa_arm64/counters.rs (move kperf + pmu)

**Files:**
- Create: `crates/platform/src/isa_arm64/counters.rs`

- [ ] **Step 1: Merge kperf.rs and pmu.rs into counters.rs**

Each section behind `#[cfg(target_os)]`. Change imports to `use crate::metering::{CycleCounter, Measurement}`. Use `core::hint::black_box` in tests instead of `std::hint::black_box`.

- [ ] **Step 2: Run tests (if on aarch64)**

Run: `cargo test -p mantis-platform --no-default-features -- counters`

- [ ] **Step 3: Commit**

```
feat(platform): add KperfCounter and PmuCounter (moved from mantis-bench)
```

---

## Task 17: metering/ and cpudetect.rs

**Files:**
- Create: `crates/platform/src/metering/mod.rs`
- Create: `crates/platform/src/metering/instant.rs`
- Create: `crates/platform/src/cpudetect.rs`

- [ ] **Step 1: Create metering/mod.rs**

Move `CycleCounter` trait, `Measurement` struct, `DefaultCounter` type alias from bench. Use `cfg_if::cfg_if!` for DefaultCounter dispatch.

- [ ] **Step 2: Create metering/instant.rs**

Move from bench. Change `use crate::counters::{...}` to `use super::{CycleCounter, Measurement}`.

- [ ] **Step 3: Create cpudetect.rs**

Extract `cpu_name()` from bench report.rs. Platform-specific: sysctl on macOS, /proc/cpuinfo on Linux.

- [ ] **Step 4: Verify full platform crate compiles and tests pass**

Run: `cargo test -p mantis-platform --features std,asm`
Run: `cargo test -p mantis-platform --no-default-features`

- [ ] **Step 5: Commit**

```
feat(platform): add metering module and cpudetect
```

---

## Task 18: Migrate mantis-core — remove CopyPolicy

**Files:**
- Modify: `crates/core/src/lib.rs`

- [ ] **Step 1: Remove CopyPolicy trait definition**

Delete the `CopyPolicy` trait (lines ~138-162).

- [ ] **Step 2: Verify mantis-core compiles**

Run: `cargo check -p mantis-core --no-default-features`

- [ ] **Step 3: Commit**

```
refactor(core): remove CopyPolicy trait (moved to mantis-platform)
```

---

## Task 19: Migrate mantis-queue — use platform

**Files:**
- Modify: `crates/queue/Cargo.toml`
- Modify: `crates/queue/src/lib.rs`
- Modify: multiple queue source files (see file map)
- Delete: `crates/queue/src/pad.rs`
- Delete: `crates/queue/src/copy_ring/raw/simd.rs`

- [ ] **Step 1: Add mantis-platform dependency, forward features**

- [ ] **Step 2: Update all import paths**

Replace `mantis_core::CopyPolicy` with `mantis_platform::CopyPolicy`, `crate::pad::CachePadded` with `mantis_platform::CachePadded`, etc.

- [ ] **Step 3: Delete moved files**

Delete `pad.rs` and `copy_ring/raw/simd.rs`.

- [ ] **Step 4: Verify queue compiles and all tests pass**

Run: `cargo test -p mantis-queue --features alloc,std`
Run: `cargo test -p mantis-queue --no-default-features`

- [ ] **Step 5: Commit**

```
refactor(queue): use mantis-platform for CachePadded, CopyPolicy, SIMD
```

---

## Task 20: Migrate mantis-bench — use platform counters

**Files:**
- Modify: `crates/bench/Cargo.toml`
- Modify: `crates/bench/src/lib.rs`
- Modify: `crates/bench/src/measurement.rs`
- Modify: `crates/bench/src/report.rs`
- Delete: `crates/bench/src/counters/` (entire directory)

- [ ] **Step 1: Update bench Cargo.toml**

Add `mantis-platform` with `features = ["std", "asm"]`. Remove `libc` target dependency.

- [ ] **Step 2: Update imports in measurement.rs and report.rs**

- [ ] **Step 3: Delete counters/ directory**

- [ ] **Step 4: Verify bench compiles and tests pass**

Run: `cargo test -p mantis-bench`

- [ ] **Step 5: Commit**

```
refactor(bench): use mantis-platform for counters and CPU detection
```

---

## Task 21: Full workspace verification

- [ ] **Step 1: Full workspace build**

Run: `cargo build --features alloc,std`
Run: `cargo +nightly build --all-features`

- [ ] **Step 2: Full workspace test**

Run: `cargo test --features alloc,std`
Run: `cargo +nightly test --all-features`

- [ ] **Step 3: no_std tests**

Run: `cargo test -p mantis-core -p mantis-types -p mantis-queue -p mantis-platform --no-default-features`

- [ ] **Step 4: Clippy**

Run: `cargo clippy --all-targets --features alloc,std -- -D warnings`
Run: `cargo +nightly clippy --all-targets --all-features -- -D warnings`

- [ ] **Step 5: Format**

Run: `cargo fmt --all --check`

- [ ] **Step 6: Miri**

Run: `cargo +nightly miri test -p mantis-platform`
Run: `cargo +nightly miri test -p mantis-queue`

- [ ] **Step 7: Cargo deny**

Run: `cargo deny check`

- [ ] **Step 8: Benchmarks still work**

Run: `cargo bench --bench spsc --features alloc,std,asm,bench-contenders -- --quick`

- [ ] **Step 9: Commit (if any fixups needed)**

```
fix(workspace): address any issues from full verification
```

---

## Task 22: Documentation updates

**Files:**
- Create: `crates/platform/README.md`
- Modify: `README.md` (workspace root)
- Modify: `CLAUDE.md`
- Modify: `docs/PROGRESS.md`
- Modify: `crates/bench/README.md`

- [ ] **Step 1: Create platform README.md**

Describe crate purpose, module layout (matching Constantine's `platforms/`), feature flags.

- [ ] **Step 2: Update root README.md**

Add `mantis-platform` to crate listing.

- [ ] **Step 3: Update CLAUDE.md workspace layout**

Add: `crates/platform/ mantis-platform  Platform abstractions, CT types, SIMD, counters  (no_std)`

- [ ] **Step 4: Update docs/PROGRESS.md**

Add mantis-platform to crate status table.

- [ ] **Step 5: Update crates/bench/README.md**

Note counters moved to `mantis-platform`.

- [ ] **Step 6: Commit**

```
docs: update documentation for mantis-platform crate
```

---

## Task 23: Final push

- [ ] **Step 1: Review all changes**

Run: `git diff --stat main`

- [ ] **Step 2: Push branch**

Run: `git push origin feat/spsc-ring-buffer`

- [ ] **Step 3: Verify CI passes**

Check all CI jobs pass on the PR.
