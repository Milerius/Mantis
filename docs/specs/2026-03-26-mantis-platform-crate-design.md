# mantis-platform Crate Design

> Consolidate all platform-specific code into a single, `no_std`-first crate
> achieving full parity with Constantine's `platforms/` module.

**Date:** 2026-03-26
**Status:** Approved (v2 — expanded to full Constantine parity)
**Crate:** `mantis-platform` at `crates/platform/`

---

## 1. Motivation

Platform-specific code is currently scattered across two crates:

- **mantis-queue** — SIMD copy kernels (`copy_ring/raw/simd.rs`), cache padding
  (`pad.rs`)
- **mantis-bench** — cycle counters (`counters/`), CPU detection (`report.rs`)

This creates several problems:

1. **Duplication risk** — future crates needing SIMD or counters must either
   depend on queue/bench or duplicate code.
2. **Wrong abstraction level** — RDTSC is an x86 instruction, not benchmark
   logic. SIMD copy is a platform primitive, not a queue detail.
3. **No central ISA config** — each file independently `cfg`-gates on
   `target_arch`, with no shared constants or detection.
4. **No constant-time foundation** — a financial SDK handling prices, orders,
   and keys needs timing-attack resistance from the ground up.

Constantine solves this with a single `platforms/` module that owns all
platform-specific code: config detection, constant-time types, bit operations,
intrinsics, extended precision, compiler hints, ISA-specific assembly, CPUID
detection, and metering. We adopt the same model with zero deferrals.

---

## 2. Design Principles

1. **`no_std` by default** — hot-path primitives compile everywhere without
   `std`. OS-dependent code (CPU detection, `InstantCounter`) behind `std`
   feature. Platform counters that use OS FFI (`KperfCounter`, `PmuCounter`)
   are gated on `target_arch` + `target_os` only — no feature flag needed
   since the target triple already implies OS availability.
2. **Compile-time dispatch only** — `cfg(target_arch)` gates, no runtime
   dispatch in hot paths. Dead branches eliminated by the compiler.
3. **ISA modules own their code** — `isa_x86/` contains all x86_64-specific
   code (SIMD, RDTSC, assembler, CPUID, cmov). `isa_arm64/` contains all
   aarch64-specific code. Portable code lives at the root or in `intrinsics/`.
4. **Leaf crate** — `mantis-platform` depends on nothing in the workspace.
   It is the bottom of the dependency graph.
5. **Traits and impls co-located** — `CopyPolicy` trait moves from
   `mantis-core` to `mantis-platform`, since it describes how to copy bytes
   between memory locations — a platform primitive.
6. **Constant-time as foundation** — `Ct<T>`, `CTBool<T>`, `Carry`, `Borrow`
   types prevent the compiler from optimizing bitwise operations into branches.
   All crypto-adjacent and price-sensitive code paths use these types.

---

## 3. Module Layout

```
crates/platform/
  Cargo.toml
  README.md
  src/
    lib.rs                      # #![no_std], feature gates, top-level re-exports
    config.rs                   # compile-time: X86_64, AARCH64, IS_MACOS, IS_LINUX, CACHE_LINE
    bithacks.rs                 # vartime bit ops: is_power_of_two, next_power_of_two, log2_floor,
                                #   trailing_zeros, round_up, ceil_div_vartime
    pad.rs                      # CachePadded<T> with #[repr(align(128))]
    constant_time/
      mod.rs                    # re-exports
      ct_types.rs               # Ct<T>, CTBool<T>, Carry, Borrow, VarTime marker
      ct_routines.rs            # bitwise ops on Ct<T>, comparisons, cneg, isZero, isNonZero, isMsbSet
      multiplexers.rs           # mux (portable + x86 cmov), ccopy, secretLookup
      ct_division.rs            # div2n1n: constant-time division
    intrinsics/
      mod.rs                    # re-exports
      copy_policy.rs            # CopyPolicy trait definition
      copy_dispatch.rs          # CopyDispatcher<T, N>, DefaultCopyPolicy, SimdCopyPolicy
      addcarry_subborrow.rs     # addC, subB with Carry/Borrow, x86 intrinsic + portable fallback
      extended_prec.rs           # mul, muladd1, muladd2, smul, mulAcc, mulDoubleAcc
      compiler_hints.rs         # prefetch, prefetch_large, PrefetchRW, PrefetchLocality
    isa_x86/
      mod.rs                    # cfg-gated re-exports (compiled only on x86_64)
      simd.rs                   # SSE2 load128/store128 + copy_16/32/48/64 kernels
      rdtsc.rs                  # RdtscCounter: lfence + rdtsc + lfence
      cpudetect.rs              # CPUID feature detection, load-time cached flags
      assembler.rs              # RM, Constraint, MemIndirectAccess, Register, Operand, Assembler_x86
    isa_arm64/
      mod.rs                    # cfg-gated re-exports (compiled only on aarch64)
      simd.rs                   # NEON vld1q_u8/vst1q_u8 + copy_16/32/48/64 kernels
      counters.rs               # KperfCounter (macOS) + PmuCounter (Linux)
      assembler.rs              # RM, ConditionCode, Constraint, MemIndirectAccess, Register, Operand
    metering/
      mod.rs                    # CycleCounter trait, DefaultCounter type alias, Measurement
      instant.rs                # InstantCounter (std feature only)
    cpudetect.rs                # cpu_name() -> String (sysctl / /proc/cpuinfo, std only)
```

### Compilation rules

- `isa_x86/` — only compiled when `cfg(target_arch = "x86_64")`
- `isa_x86/rdtsc.rs` — additionally requires `feature = "asm"` + `feature = "std"`
- `isa_x86/cpudetect.rs` — requires `feature = "std"` (CPUID inline asm is
  unconditional on x86_64, but the caching infra needs `std::sync::OnceLock`)
- `isa_arm64/` — only compiled when `cfg(target_arch = "aarch64")`
- `isa_arm64/counters.rs` — KperfCounter gated on `target_os = "macos"`,
  PmuCounter gated on `target_os = "linux"` (no feature flag needed)
- `constant_time/multiplexers.rs` — portable fallback always available;
  x86 cmov assembly behind `cfg(target_arch = "x86_64")`
- `metering/instant.rs`, `cpudetect.rs` — only compiled when `feature = "std"`
- `intrinsics/copy_dispatch.rs` (`SimdCopyPolicy`) — only when `feature = "nightly"`
- Everything else — compiles on all targets

---

## 4. Public API

### 4.1 `config.rs`

```rust
pub const X86_64: bool = cfg!(target_arch = "x86_64");
pub const AARCH64: bool = cfg!(target_arch = "aarch64");
pub const IS_MACOS: bool = cfg!(target_os = "macos");
pub const IS_LINUX: bool = cfg!(target_os = "linux");
pub const CACHE_LINE: usize = 128;
```

### 4.2 `bithacks.rs`

```rust
pub const fn is_power_of_two(n: usize) -> bool;
pub const fn next_power_of_two(n: usize) -> usize;
pub const fn log2_floor(n: usize) -> u32;
pub const fn trailing_zeros(n: usize) -> u32;
pub const fn round_up(value: usize, alignment: usize) -> usize;
pub const fn ceil_div_vartime(a: usize, b: usize) -> usize;
```

All bit operations centralized here as the SDK's canonical source for these
primitives. All `const fn`. Semantically vartime (not constant-time). Follows
Constantine's `bithacks.nim` pattern.

### 4.3 `pad.rs`

Preserves the existing API from `queue/src/pad.rs`:

```rust
#[repr(align(128))]
pub struct CachePadded<T> { value: T }
impl<T> CachePadded<T> { pub const fn new(value: T) -> Self; }
// Deref, DerefMut, Debug impls
```

### 4.4 `constant_time/ct_types.rs`

Maps directly from Constantine's `ct_types.nim`:

```rust
/// Constant-time unsigned integer. Prevents compiler from optimizing
/// bitwise operations into branches.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Ct<T>(T);

/// Constant-time boolean. Range restricted to 0 or 1.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CTBool<T>(Ct<T>);

/// Carry flag for addition chains.
pub type Carry = Ct<u8>;

/// Borrow flag for subtraction chains.
pub type Borrow = Ct<u8>;

/// Marker type for variable-time operations (effect tracking).
pub struct VarTime;
```

`Ct<T>` wraps unsigned integers and provides all arithmetic/bitwise
operations through explicit trait implementations that prevent the compiler
from introducing conditional branches. This is the foundation for all
timing-attack-resistant code.

### 4.5 `constant_time/ct_routines.rs`

Maps from Constantine's `ct_routines.nim`:

```rust
// Constructors
impl<T> Ct<T> {
    pub fn ct(val: T) -> Self;
}
impl<T> CTBool<Ct<T>> {
    pub fn ctrue() -> Self;
    pub fn cfalse() -> Self;
}

// Arithmetic on Ct<T>: and, or, xor, not, +, -, shr, shl, *
// Comparisons returning CTBool<T>: ==, <, <=, !=
// Utilities: isMsbSet, isZero, isNonZero, cneg
```

All operations are implemented via bitwise manipulation to prevent branch
generation.

### 4.6 `constant_time/multiplexers.rs`

Maps from Constantine's `multiplexers.nim`:

```rust
/// Constant-time select: returns x if ctl is true, y otherwise.
pub fn mux<T>(ctl: CTBool<T>, x: Ct<T>, y: Ct<T>) -> Ct<T>;

/// Constant-time conditional copy: copies y into x if ctl is true.
pub fn ccopy<T>(ctl: CTBool<T>, x: &mut Ct<T>, y: Ct<T>);

/// Constant-time table lookup (scans entire table).
pub fn secret_lookup<T>(table: &[T], index: Ct<usize>) -> T;
```

Portable fallback: `y ^ (-T(ctl) & (x ^ y))`.
x86_64: `test + cmovz` inline assembly (matching Constantine's `mux_x86`).

### 4.7 `constant_time/ct_division.rs`

Maps from Constantine's `ct_division.nim`:

```rust
/// Constant-time division of a double-width number by a single-width divisor.
/// (quotient, remainder) <- (n_hi, n_lo) / d
pub fn div2n1n<T>(n_hi: Ct<T>, n_lo: Ct<T>, d: Ct<T>) -> (Ct<T>, Ct<T>);
```

Implemented using the constant-time binary shift algorithm from BearSSL.

### 4.8 `intrinsics/addcarry_subborrow.rs`

Maps from Constantine's `addcarry_subborrow.nim`:

```rust
/// Addition with carry: (carry_out, sum) <- a + b + carry_in
pub fn add_c(a: Ct<u32>, b: Ct<u32>, carry_in: Carry) -> (Ct<u32>, Carry);
pub fn add_c(a: Ct<u64>, b: Ct<u64>, carry_in: Carry) -> (Ct<u64>, Carry);

/// Subtraction with borrow: (borrow_out, diff) <- a - b - borrow_in
pub fn sub_b(a: Ct<u32>, b: Ct<u32>, borrow_in: Borrow) -> (Ct<u32>, Borrow);
pub fn sub_b(a: Ct<u64>, b: Ct<u64>, borrow_in: Borrow) -> (Ct<u64>, Borrow);
```

x86_64: uses `_addcarry_u64` / `_subborrow_u64` intrinsics.
Portable: widening arithmetic fallback (matching Constantine's `else` branch).

Note: Rust doesn't have function overloading. We use a trait-based approach:

```rust
pub trait AddCarryOp: Sized {
    fn add_c(self, rhs: Self, carry_in: Carry) -> (Self, Carry);
}
pub trait SubBorrowOp: Sized {
    fn sub_b(self, rhs: Self, borrow_in: Borrow) -> (Self, Borrow);
}
```

### 4.9 `intrinsics/extended_prec.rs`

Maps from Constantine's `extended_precision.nim`. Full set:

```rust
/// Widening multiply: (hi, lo) <- a * b
pub trait WideMul: Sized {
    fn wide_mul(self, rhs: Self) -> (Self, Self);
}

/// Widening multiply-add: (hi, lo) <- a * b + c
pub trait WideMulAdd1: WideMul {
    fn muladd1(self, rhs: Self, c: Self) -> (Self, Self);
}

/// Widening multiply-add-add: (hi, lo) <- a * b + c1 + c2
pub trait WideMulAdd2: WideMul {
    fn muladd2(self, rhs: Self, c1: Self, c2: Self) -> (Self, Self);
}

/// Signed widening multiply: (hi, lo) <- a * b (signed)
pub trait SignedWideMul: Sized {
    fn smul(self, rhs: Self) -> (Self, Self);
}

/// 3-limb accumulate: (t, u, v) += a * b
pub fn mul_acc<T: WideMul + AddCarryOp>(t: &mut T, u: &mut T, v: &mut T, a: T, b: T);

/// 3-limb double accumulate: (t, u, v) += 2 * a * b
pub fn mul_double_acc<T: WideMul + AddCarryOp>(t: &mut T, u: &mut T, v: &mut T, a: T, b: T);
```

Portable impls for `Ct<u32>` (via u64) and `Ct<u64>` (via u128).

### 4.10 `intrinsics/compiler_hints.rs`

Maps from Constantine's `compiler_optim_hints.nim` + `primitives.nim` prefetch:

```rust
/// Prefetch direction.
#[repr(i32)]
pub enum PrefetchRW { Read = 0, Write = 1 }

/// Prefetch temporal locality hint.
#[repr(i32)]
pub enum PrefetchLocality {
    NoTemporal = 0,
    Low = 1,
    Moderate = 2,
    High = 3,
}

/// Prefetch a cache line containing `ptr`.
pub fn prefetch<T>(ptr: *const T, rw: PrefetchRW, locality: PrefetchLocality);

/// Prefetch a large value spanning multiple cache lines.
pub fn prefetch_large<T>(ptr: *const T, rw: PrefetchRW, locality: PrefetchLocality, max_lines: usize);
```

On GCC-compatible compilers (Rust uses LLVM), maps to `core::arch::x86_64::_mm_prefetch`
on x86 or `core::arch::aarch64::__prefetch` on ARM. Fallback: no-op.

### 4.11 `intrinsics/copy_policy.rs`

```rust
pub trait CopyPolicy<T: Copy> {
    fn copy_in(dst: *mut T, src: *const T);
    fn copy_out(dst: *mut T, src: *const T);
}
```

Moved from `mantis-core`.

### 4.12 `intrinsics/copy_dispatch.rs`

```rust
pub(crate) struct CopyDispatcher<T, const N: usize>;
pub struct DefaultCopyPolicy;
#[cfg(feature = "nightly")]
pub struct SimdCopyPolicy;
```

Note: SIMD kernel code (load128, store128, copy_N, copy_bucket) remains in
this file with `#[cfg(target_arch)]` guards due to tight coupling between
dispatch macros and platform-specific types. ISA simd.rs files are placeholders
for future AVX2/SVE kernels. This is tracked technical debt — a FIXME comment
must be present.

### 4.13 `isa_x86/cpudetect.rs`

Maps from Constantine's `cpudetect_x86.nim`:

```rust
/// Query CPUID leaf.
fn cpuid(eax: u32, ecx: u32) -> CpuIdRegs;

/// CPU model name via CPUID leaves 0x80000002-4.
pub fn cpu_name_x86() -> String;

// Feature flags — cached in static atomics, initialized on first access.
// Uses OnceLock for thread-safe lazy init (~70 cycle CPUID cost amortized).
pub fn has_sse2() -> bool;
pub fn has_sse3() -> bool;
pub fn has_ssse3() -> bool;
pub fn has_sse41() -> bool;
pub fn has_sse42() -> bool;
pub fn has_avx() -> bool;
pub fn has_avx2() -> bool;
pub fn has_avx512f() -> bool;
pub fn has_avx512bw() -> bool;
pub fn has_avx512dq() -> bool;
pub fn has_avx512vl() -> bool;
pub fn has_fma3() -> bool;
pub fn has_bmi1() -> bool;
pub fn has_bmi2() -> bool;
pub fn has_adx() -> bool;     // ADCX/ADOX — critical for multi-limb arithmetic
pub fn has_aes() -> bool;
pub fn has_clmul() -> bool;   // Carry-less multiplication
pub fn has_popcnt() -> bool;
pub fn has_sha() -> bool;
pub fn has_gfni() -> bool;    // Galois Field New Instruction
```

Key design decision from Constantine: results cached in `static` variables
initialized at first access via `OnceLock`, NOT queried per-call. CPUID is
~70 cycles / ~120 latency and cannot be in hot paths.

### 4.14 `isa_x86/assembler.rs`

Maps from Constantine's `macro_assembler_x86_att.nim`:

```rust
/// Register or Memory operand kind.
pub enum RM {
    Reg, Mem, Imm, MemOffsettable,
    PointerInReg, ElemsInReg,
    RCX, RDX, R8, RAX,
    CarryFlag, ClobberedReg,
}

/// GCC extended assembly constraint modifier.
pub enum Constraint {
    Input, InputCommutative,
    OutputOverwrite, OutputEarlyClobber,
    InputOutput, InputOutputEarlyClobber,
    ClobberedRegister,
}

/// Memory indirect access mode.
pub enum MemIndirectAccess {
    NoAccess, Read, Write, ReadWrite,
}

/// x86_64 general-purpose registers.
pub enum Register { Rbx, Rdx, R8, Rax, Xmm0 }

/// Operand kind discriminant.
pub enum OpKind { Register, FromArray, ArrayAddr, Array2dAddr }

/// Assembly operand (discriminated).
pub enum Operand { ... }

/// x86_64 assembler state.
pub struct AssemblerX86 {
    pub code: String,
    pub word_bit_width: usize,
    // operand tracking
}
```

### 4.15 `isa_arm64/assembler.rs`

Maps from Constantine's `macro_assembler_arm64.nim`:

```rust
pub enum RM {
    Reg, Mem, Imm, MemOffsettable,
    PointerInReg, ElemsInReg,
    XZR, CarryFlag, BorrowFlag, ClobberedReg,
}

/// ARM64 condition codes.
pub enum ConditionCode {
    Eq, Ne, Cs, Hs, Cc, Lo, Mi, Pl,
    Vs, Vc, Hi, Ls, Ge, Lt, Gt, Le, Al,
}

pub enum Constraint {
    Input, InputCommutative,
    OutputOverwrite, OutputEarlyClobber,
    InputOutput, InputOutputEarlyClobber,
    ClobberedRegister,
}

pub enum MemIndirectAccess { NoAccess, Read, Write, ReadWrite }

pub enum Register { Xzr }

pub enum OpKind { Register, FromArray, ArrayAddr, Array2dAddr }
pub enum Operand { ... }
```

Note: ARM64 has `BorrowFlag` (inverted carry semantics for subtraction) which
x86 does not — this is architecturally significant for multi-limb arithmetic.

### 4.16 `metering/mod.rs`

```rust
pub struct Measurement { pub nanos: u64, pub cycles: u64 }
pub trait CycleCounter: Send + Sync {
    fn start(&self) -> u64;
    fn elapsed(&self, start: u64) -> Measurement;
}
// DefaultCounter type alias — cfg-dispatched per platform
```

### 4.17 `cpudetect.rs`

```rust
#[cfg(feature = "std")]
pub fn cpu_name() -> String;
```

---

## 5. Feature Flags

```toml
[features]
default = []
std = []
asm = []
nightly = []
```

No `alloc` feature — nothing in the crate currently needs heap allocation.

### External dependencies

```toml
[dependencies]
cfg-if = { workspace = true }

[target.'cfg(all(target_arch = "aarch64", target_os = "linux"))'.dependencies]
libc = { workspace = true, default-features = false }
```

### DefaultCounter dispatch

```rust
cfg_if::cfg_if! {
    if #[cfg(all(target_arch = "x86_64", feature = "asm", feature = "std"))] {
        pub type DefaultCounter = isa_x86::rdtsc::RdtscCounter;
    } else if #[cfg(all(target_arch = "aarch64", target_os = "macos"))] {
        pub type DefaultCounter = isa_arm64::KperfCounter;
    } else if #[cfg(all(target_arch = "aarch64", target_os = "linux"))] {
        pub type DefaultCounter = isa_arm64::PmuCounter;
    } else if #[cfg(feature = "std")] {
        pub type DefaultCounter = InstantCounter;
    }
}
```

---

## 6. Dependency Graph

```
mantis-platform  (leaf — zero in-workspace deps, owns CopyPolicy + Ct<T>)
    ^         ^
    |         |
mantis-core   |   (traits: IndexStrategy, PushPolicy, Instrumentation)
    ^         |
    |         |
mantis-types  |   (newtypes: SeqNum, SlotIndex, QueueError — no workspace deps)
    ^    ^    |
    |    |    |
    +----+----+
         |
   mantis-queue   (depends on platform + core + types)
         ^
         |
   mantis-bench   (depends on platform for counters + CPU detect)
```

---

## 7. Migration Map

### Code that moves

| Current location | Destination | Notes |
|---|---|---|
| `queue/src/copy_ring/raw/simd.rs` (x86_64 code) | `platform/src/intrinsics/copy_dispatch.rs` | With cfg guards |
| `queue/src/copy_ring/raw/simd.rs` (aarch64 code) | `platform/src/intrinsics/copy_dispatch.rs` | With cfg guards |
| `queue/src/copy_ring/raw/simd.rs` (CopyDispatcher) | `platform/src/intrinsics/copy_dispatch.rs` | Dispatch logic |
| `core/src/lib.rs` (CopyPolicy trait) | `platform/src/intrinsics/copy_policy.rs` | Trait definition moves |
| `queue/src/pad.rs` | `platform/src/pad.rs` | Direct move, preserve API |
| `bench/src/counters/rdtsc.rs` | `platform/src/isa_x86/rdtsc.rs` | Direct move |
| `bench/src/counters/kperf.rs` | `platform/src/isa_arm64/counters.rs` | Merged, cfg by OS |
| `bench/src/counters/pmu.rs` | `platform/src/isa_arm64/counters.rs` | Merged, cfg by OS |
| `bench/src/counters/instant.rs` | `platform/src/metering/instant.rs` | Direct move |
| `bench/src/counters/mod.rs` | `platform/src/metering/mod.rs` | Direct move |
| `bench/src/report.rs` (cpu_name fn) | `platform/src/cpudetect.rs` | Extracted |

### Code that stays (with import path updates)

| Location | Import changes |
|---|---|
| `core/src/lib.rs` | Remove CopyPolicy definition |
| `queue/src/engine.rs` | Import CachePadded from platform |
| `queue/src/copy_ring/engine.rs` | Import CopyPolicy from platform |
| `bench/src/measurement.rs` | Import CycleCounter, Measurement from platform |
| `bench/src/report.rs` | Call platform::cpudetect::cpu_name() |

---

## 8. Testing Strategy

### Unit tests (in-crate)

| Module | Tests |
|---|---|
| `bithacks.rs` | is_power_of_two, next_power_of_two, log2_floor, trailing_zeros, round_up, ceil_div_vartime. Edge cases: 0, 1, usize::MAX. |
| `pad.rs` | Alignment: `align_of::<CachePadded<u8>>() == 128`. Deref, DerefMut, new(). |
| `constant_time/ct_types.rs` | Ct<T> construction, Carry/Borrow are Ct<u8>, VarTime is ZST. |
| `constant_time/ct_routines.rs` | All bitwise ops, comparisons (==, <, <=), isZero, isNonZero, isMsbSet, cneg. Property: constant-time comparisons match stdlib comparisons for all tested values. |
| `constant_time/multiplexers.rs` | mux: all four (ctl, x, y) combos. ccopy: conditional vs no-op. secret_lookup: correct index, scans full table. x86 cmov path (if on x86). |
| `constant_time/ct_division.rs` | div2n1n: basic division, remainder, edge cases (max values, divisor = 1). Cross-check against stdlib division. |
| `intrinsics/addcarry_subborrow.rs` | addC: no carry, carry in, carry out, carry chain. subB: same pattern with borrows. u32 and u64. |
| `intrinsics/extended_prec.rs` | wide_mul: u32 * u32 -> u64, u64 * u64 -> u128, max * max. muladd1, muladd2: no-overflow invariant. smul: signed correctness. mulAcc, mulDoubleAcc: accumulate correctness. |
| `intrinsics/compiler_hints.rs` | prefetch: doesn't crash (no-op on non-GCC). prefetch_large: handles sizeof(T) > cache line. |
| `intrinsics/copy_dispatch.rs` | Roundtrip tests for all size buckets. |
| `isa_x86/cpudetect.rs` | cpu_name_x86 returns non-empty. Feature flags return bools (no crash). has_sse2 is true on all x86_64. |
| `isa_x86/assembler.rs` | All enum variants constructable. Operand construction. AssemblerX86 construction. |
| `isa_arm64/assembler.rs` | All enum variants constructable. ConditionCode variants. |
| `metering/` | Each counter: start/elapsed produces non-zero nanos. |

### Cross-crate verification

- All `mantis-queue` tests pass (same behavior, different import paths).
- All `mantis-bench` benchmarks produce identical JSON reports.
- Miri passes on `mantis-platform` (SIMD tests excluded via existing cfg).

---

## 9. Documentation Updates

- **`CLAUDE.md`** — add `mantis-platform` to workspace layout table
- **`README.md`** — add platform crate to crate listing
- **`crates/platform/README.md`** — new, describes crate purpose and usage
- **`crates/bench/README.md`** — update to reflect counters moved to platform
- **`docs/PROGRESS.md`** — add mantis-platform to crate status table

---

## 10. Future Extensions

The module layout supports these but they are not in initial scope:

- **256-bit / 512-bit SIMD kernels** — AVX2/AVX-512 copy kernels for larger
  types. Extends `isa_x86/simd.rs`.
- **ARM SVE support** — scalable vector extensions. Extends `isa_arm64/simd.rs`.
- **Full macro assembler codegen** — the type definitions (Constraint, RM,
  Operand, Assembler) are in place; the actual `asm!` code generation logic
  comes when the first consumer (e.g., Montgomery multiplication) needs it.
