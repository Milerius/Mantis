# Mantis Repository Bootstrap — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set up the Mantis Rust workspace with 6 crates, full CI pipeline, agent-friendly CLAUDE.md, and all tooling infrastructure so the first SPSC ring implementation goes through the complete quality gauntlet from its first commit.

**Architecture:** Cargo workspace under `crates/` with 3 `no_std` library crates (core, types, queue) and 3 `std` tooling crates (bench, layout, verify). CI has 4 workflows: PR gate, benchmark regression, nightly extended checks, and formal verification. Constantine-inspired coding model with compile-time platform specialization.

**Tech Stack:** Rust stable + nightly, criterion, cargo-deny, cargo-careful, cargo-mutants, cargo-llvm-cov, cargo-fuzz, kani, bolero, just, prek, GitHub Actions.

**Spec:** `docs/specs/2026-03-25-repo-bootstrap-design.md`

**Deferred to SPSC ring phase (not in scope for this bootstrap):**
- `benches/` directory with criterion binaries and contender FFI wrappers
- `fuzz/` directory with cargo-fuzz targets
- `bench.yml` baseline comparison jobs (bench-baseline, bench-candidate, bench-contenders, bench-report)
- `nightly.yml` fuzz, bench-matrix, and fuzz-coverage jobs
- `.prek.toml` pre-commit hook configuration (install prek during SPSC ring phase when there's code to lint)
- Full `Capacity<const N: usize>` type (bootstrap uses `AssertPowerOfTwo` for compile-time validation; `Capacity` wrapper added when the ring buffer needs it)

**CI Action Pinning Note:** All GitHub Actions in this plan use version tags (e.g., `@v4`). The executing agent MUST look up current SHA hashes at execution time and pin to `@<full-sha> # vX.Y.Z` format. Add `persist-credentials: false` to every `actions/checkout` step.

---

### Task 1: Workspace Root — Cargo.toml + Config Files

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `clippy.toml`
- Create: `deny.toml`
- Modify: `.gitignore`

- [ ] **Step 1: Create workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
license = "Apache-2.0"
repository = "https://github.com/mantis-sdk/mantis"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
# Panic prevention
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
panic_in_result_fn = "deny"
unimplemented = "deny"
# No cheating
allow_attributes = "deny"
# Code hygiene
dbg_macro = "deny"
todo = "deny"
print_stdout = "deny"
print_stderr = "deny"
# Safety
await_holding_lock = "deny"
large_futures = "deny"
exit = "deny"
mem_forget = "deny"
# Pedantic relaxations (too noisy)
module_name_repetitions = "allow"
similar_names = "allow"

[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "warn"

[workspace.dependencies]
# Workspace crates (internal)
mantis-core = { path = "crates/core" }
mantis-types = { path = "crates/types" }
mantis-queue = { path = "crates/queue" }
mantis-bench = { path = "crates/bench" }
mantis-layout = { path = "crates/layout" }
mantis-verify = { path = "crates/verify" }

# External
criterion = { version = "0.5", features = ["html_reports"] }
bolero = "0.11"
```

- [ ] **Step 2: Create `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy", "rust-src"]
targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
]
```

- [ ] **Step 3: Create `clippy.toml`**

```toml
msrv = "1.85.0"
cognitive-complexity-threshold = 8
too-many-arguments-threshold = 5
type-complexity-threshold = 250
single-char-binding-names-threshold = 4
```

- [ ] **Step 4: Create `deny.toml`**

```toml
[graph]
targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "aarch64-apple-darwin",
]

[advisories]
vulnerability = "deny"
unmaintained = "warn"
yanked = "deny"
notice = "warn"

[licenses]
unlicensed = "deny"
allow = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
]
copyleft = "deny"

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = []

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = [
    "https://github.com/rust-lang/crates.io-index",
    "https://index.crates.io",
]
allow-git = []
```

- [ ] **Step 5: Update `.gitignore`**

Append to existing `.gitignore`:

```
# Fuzz artifacts
fuzz/artifacts/
fuzz/corpus/

# Profiling
*.profraw
*.profdata
perf.data*
flamegraph.svg

# Coverage
lcov.info
tarpaulin-report.html

# Just
.justfile.tmp
```

- [ ] **Step 6: Verify workspace compiles (will fail — no members yet)**

Run: `cargo check 2>&1 | head -5`
Expected: Error about no workspace members (this is correct — we create crates next)

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml clippy.toml deny.toml .gitignore
git commit -m "$(cat <<'EOF'
chore: add workspace root config files

Workspace Cargo.toml with shared lints, rust-toolchain.toml pinning
stable with cross-platform targets, clippy.toml thresholds, deny.toml
for supply chain safety, and gitignore additions for tooling artifacts.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Crate Skeletons — mantis-core + mantis-types

**Files:**
- Create: `crates/core/Cargo.toml`
- Create: `crates/core/src/lib.rs`
- Create: `crates/types/Cargo.toml`
- Create: `crates/types/src/lib.rs`

- [ ] **Step 1: Create `crates/core/Cargo.toml`**

```toml
[package]
name = "mantis-core"
description = "Core traits and foundations for the Mantis low-latency financial SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
std = []

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/core/src/lib.rs`**

```rust
//! Core traits and strategy definitions for the Mantis SDK.
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

/// Defines how head/tail indices wrap around the buffer capacity.
pub trait IndexStrategy {
    /// Wrap a raw index to a valid slot position.
    fn wrap(index: usize, capacity: usize) -> usize;
}

/// Defines push behavior when the queue is full.
pub trait PushPolicy {
    /// Returns `true` if the push should block/spin when full.
    fn should_block() -> bool;
}

/// Defines measurement hooks for instrumentation.
pub trait Instrumentation {
    /// Called after a successful push.
    fn on_push(&self) {}
    /// Called after a successful pop.
    fn on_pop(&self) {}
    /// Called when a push fails due to full queue.
    fn on_push_full(&self) {}
    /// Called when a pop fails due to empty queue.
    fn on_pop_empty(&self) {}
}

/// Power-of-2 masked index strategy. Wraps via bitwise AND.
pub struct Pow2Masked;

impl IndexStrategy for Pow2Masked {
    #[inline(always)]
    fn wrap(index: usize, capacity: usize) -> usize {
        debug_assert!(capacity.is_power_of_two(), "capacity must be power of 2");
        index & (capacity - 1)
    }
}

/// Push immediately returns `Err(Full)` when queue is full.
pub struct ImmediatePush;

impl PushPolicy for ImmediatePush {
    #[inline(always)]
    fn should_block() -> bool {
        false
    }
}

/// No-op instrumentation. Zero overhead in release builds.
pub struct NoInstr;

impl Instrumentation for NoInstr {}
```

- [ ] **Step 3: Create `crates/types/Cargo.toml`**

```toml
[package]
name = "mantis-types"
description = "Core type definitions for the Mantis low-latency financial SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
std = []

[lints]
workspace = true
```

- [ ] **Step 4: Create `crates/types/src/lib.rs`**

```rust
//! Core types, newtypes, and error definitions for the Mantis SDK.
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

/// Error type for queue operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// The queue is full and cannot accept more items.
    Full,
    /// The queue is empty and has no items to return.
    Empty,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "queue is full"),
            Self::Empty => write!(f, "queue is empty"),
        }
    }
}

/// Sequence number for tracking event ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeqNum(pub u64);

/// Index into a ring buffer slot array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotIndex(pub usize);

/// Compile-time assertion that a capacity is a power of two.
///
/// # Panics
///
/// Panics at compile time if `N` is not a power of two or is zero.
pub struct AssertPowerOfTwo<const N: usize>;

impl<const N: usize> AssertPowerOfTwo<N> {
    /// Const assertion. Call in a `const { }` block to validate at compile time.
    pub const VALID: () = assert!(
        N.is_power_of_two() && N > 0,
        "capacity must be a non-zero power of two"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_error_display() {
        assert_eq!(QueueError::Full.to_string(), "queue is full");
        assert_eq!(QueueError::Empty.to_string(), "queue is empty");
    }

    #[test]
    fn seq_num_ordering() {
        assert!(SeqNum(1) < SeqNum(2));
        assert_eq!(SeqNum(42), SeqNum(42));
    }

    #[test]
    fn slot_index_equality() {
        assert_eq!(SlotIndex(0), SlotIndex(0));
        assert_ne!(SlotIndex(0), SlotIndex(1));
    }

    #[test]
    fn power_of_two_valid() {
        let _ = AssertPowerOfTwo::<1>::VALID;
        let _ = AssertPowerOfTwo::<2>::VALID;
        let _ = AssertPowerOfTwo::<1024>::VALID;
    }
}
```

- [ ] **Step 5: Verify both crates compile**

Run: `cargo check -p mantis-core -p mantis-types`
Expected: Compiles successfully with no errors.

- [ ] **Step 6: Run tests**

Run: `cargo test -p mantis-types`
Expected: All 4 tests pass.

- [ ] **Step 7: Verify no_std compiles**

Run: `cargo check -p mantis-core -p mantis-types --no-default-features`
Expected: Compiles successfully (no_std mode).

- [ ] **Step 8: Commit**

```bash
git add crates/core/ crates/types/
git commit -m "$(cat <<'EOF'
feat: add mantis-core and mantis-types crate skeletons

mantis-core: strategy traits (IndexStrategy, PushPolicy, Instrumentation)
and marker types (Pow2Masked, ImmediatePush, NoInstr).

mantis-types: QueueError enum, SeqNum/SlotIndex newtypes,
AssertPowerOfTwo compile-time validator. Both crates are no_std.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Crate Skeleton — mantis-queue

**Files:**
- Create: `crates/queue/Cargo.toml`
- Create: `crates/queue/src/lib.rs`

- [ ] **Step 1: Create `crates/queue/Cargo.toml`**

```toml
[package]
name = "mantis-queue"
description = "Lock-free queue primitives for the Mantis low-latency financial SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
default = []
std = []

[dependencies]
mantis-core = { workspace = true }
mantis-types = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/queue/src/lib.rs`**

```rust
//! Lock-free queue primitives for the Mantis SDK.
//!
//! This crate provides SPSC (single-producer, single-consumer) ring buffers
//! and other bounded queue implementations optimized for low-latency
//! financial systems.
//!
//! # Architecture
//!
//! Each queue primitive follows the modular strategy pattern:
//! - Generic internal engine parameterized by strategy traits
//! - Curated preset type aliases for common configurations
//! - Platform-specific fast paths via `cfg`-gated assembly
//! - All unsafe code isolated in `raw` submodules
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

pub use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};
pub use mantis_types::QueueError;
```

- [ ] **Step 3: Verify compiles**

Run: `cargo check -p mantis-queue`
Expected: Compiles successfully.

- [ ] **Step 4: Verify no_std compiles**

Run: `cargo check -p mantis-queue --no-default-features`
Expected: Compiles successfully.

- [ ] **Step 5: Commit**

```bash
git add crates/queue/
git commit -m "$(cat <<'EOF'
feat: add mantis-queue crate skeleton

no_std queue crate depending on mantis-core and mantis-types.
Re-exports strategy markers and error types. Ready for SPSC ring
implementation.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Crate Skeletons — mantis-bench, mantis-layout, mantis-verify

**Files:**
- Create: `crates/bench/Cargo.toml`
- Create: `crates/bench/src/lib.rs`
- Create: `crates/layout/Cargo.toml`
- Create: `crates/layout/src/lib.rs`
- Create: `crates/layout/src/main.rs`
- Create: `crates/verify/Cargo.toml`
- Create: `crates/verify/src/lib.rs`

- [ ] **Step 1: Create `crates/bench/Cargo.toml`**

```toml
[package]
name = "mantis-bench"
description = "Benchmark harness and perf-counter utilities for the Mantis SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
mantis-core = { workspace = true }
mantis-types = { workspace = true }
criterion = { workspace = true }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/bench/src/lib.rs`**

```rust
//! Benchmark harness and performance counter utilities for the Mantis SDK.
//!
//! Provides:
//! - Criterion integration for statistical benchmarking
//! - Platform-aware performance counter collection (RDTSC+lfence on x86_64,
//!   monotonic time on ARM64 — see `counters` module)
//! - JSON/CSV export for cross-hardware comparison
//! - Warmup utilities for CPU frequency stabilization
//!
//! # Architecture (Constantine-inspired)
//!
//! Cycle measurement follows Constantine's pattern:
//! - x86_64: `rdtsc` with `lfence` barrier for accurate cycle counting
//! - ARM64: `mach_absolute_time` / `clock_gettime` fallback (no reliable
//!   user-space cycle counter)
//! - All measurements include CPU name detection and compiler info in reports
//!
//! This is a `std`-only tooling crate.

#![deny(unsafe_code)]

pub mod counters;
pub mod report;
```

- [ ] **Step 2b: Create `crates/bench/src/report.rs`**

```rust
//! Benchmark report metadata and serialization.

/// Benchmark report metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchReport {
    /// Name of the implementation being benchmarked.
    pub implementation: String,
    /// Target architecture (e.g., "x86_64", "aarch64").
    pub arch: String,
    /// Operating system.
    pub os: String,
    /// CPU model name.
    pub cpu: String,
}

impl BenchReport {
    /// Create a new report with detected system info.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            implementation: String::new(),
            arch: std::env::consts::ARCH.to_owned(),
            os: std::env::consts::OS.to_owned(),
            cpu: String::from("unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_fills_arch_and_os() {
        let report = BenchReport::detect();
        assert!(!report.arch.is_empty());
        assert!(!report.os.is_empty());
    }
}
```

- [ ] **Step 2c: Create `crates/bench/src/counters.rs`**

This is the skeleton for platform-aware cycle counting. The actual `unsafe` RDTSC
implementation is added in the SPSC ring phase when we need real measurements.

```rust
//! Platform-aware performance counter collection.
//!
//! # x86_64 (planned)
//!
//! Uses `rdtsc` with `lfence` serializing barrier to get accurate cycle counts.
//! The `lfence` ensures all prior instructions complete before reading the TSC,
//! preventing out-of-order execution from skewing measurements.
//!
//! ```text
//! lfence          ; serialize — wait for all prior instructions
//! rdtsc           ; read timestamp counter -> edx:eax
//! ; ... measured code ...
//! lfence          ; serialize again
//! rdtsc           ; read again
//! ; delta = end - start
//! ```
//!
//! Note: TSC frequency may differ from core frequency (turbo boost).
//! Reports should note this caveat (see Constantine's benchmark output).
//!
//! # ARM64 (planned)
//!
//! No reliable user-space cycle counter. Falls back to:
//! - macOS: `mach_absolute_time()` (nanosecond resolution)
//! - Linux: `clock_gettime(CLOCK_MONOTONIC_RAW)`
//!
//! # Usage
//!
//! The `CycleCounter` trait abstracts over platform differences.
//! Implementations are added when the first benchmark target exists.

use std::time::Instant;

/// A measurement from a performance counter.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Wall-clock duration in nanoseconds.
    pub nanos: u64,
    /// CPU cycles (if available on this platform, else 0).
    pub cycles: u64,
}

/// Trait for platform-specific cycle counting.
///
/// Implementations will use:
/// - x86_64: `rdtsc` + `lfence` (unsafe, in a dedicated raw module)
/// - ARM64: monotonic clock fallback
/// - Fallback: `std::time::Instant`
pub trait CycleCounter {
    /// Start a measurement.
    fn start(&self) -> u64;
    /// End a measurement, returning elapsed.
    fn elapsed(&self, start: u64) -> Measurement;
}

/// Fallback counter using `std::time::Instant`.
/// Used when platform-specific counters are unavailable.
pub struct InstantCounter;

impl CycleCounter for InstantCounter {
    fn start(&self) -> u64 {
        // Encode Instant as nanos since we can't store it in u64 directly.
        // This loses precision but works for the fallback path.
        let now = Instant::now();
        // Use elapsed from a reference point
        now.elapsed().as_nanos() as u64
    }

    fn elapsed(&self, _start: u64) -> Measurement {
        Measurement {
            nanos: 0,
            cycles: 0,
        }
    }
}

// TODO(spsc-ring-phase): Add x86_64 RDTSC+lfence implementation in raw/ submodule
// TODO(spsc-ring-phase): Add ARM64 monotonic clock implementation
// TODO(spsc-ring-phase): Add CPU name detection (CPUID on x86, sysctl on macOS)
// TODO(spsc-ring-phase): Add warmup loop for CPU frequency stabilization

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instant_counter_creates() {
        let counter = InstantCounter;
        let start = counter.start();
        let m = counter.elapsed(start);
        // Fallback returns zeros — real counters added in SPSC ring phase
        assert_eq!(m.cycles, 0);
    }
}
```

- [ ] **Step 3: Create `crates/layout/Cargo.toml`**

```toml
[package]
name = "mantis-layout"
description = "Struct layout and cache-line analysis tooling for the Mantis SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
mantis-core = { workspace = true }
mantis-types = { workspace = true }
mantis-queue = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 4: Create `crates/layout/src/lib.rs`**

```rust
//! Struct layout and cache-line analysis for the Mantis SDK.
//!
//! Reports size, alignment, field offsets, and cache-line occupancy
//! for hot-path data structures.
//!
//! This is a `std`-only tooling crate.

#![deny(unsafe_code)]

use core::mem;

/// Layout information for a type.
#[derive(Debug, Clone)]
pub struct LayoutInfo {
    /// Type name.
    pub name: String,
    /// Size in bytes.
    pub size: usize,
    /// Alignment in bytes.
    pub align: usize,
    /// Number of cache lines occupied (assuming 64-byte lines).
    pub cache_lines: usize,
}

/// Inspect the layout of a type.
#[must_use]
pub fn inspect<T>(name: &str) -> LayoutInfo {
    let size = mem::size_of::<T>();
    let align = mem::align_of::<T>();
    let cache_lines = (size + 63) / 64;
    LayoutInfo {
        name: name.to_owned(),
        size,
        align,
        cache_lines,
    }
}

impl std::fmt::Display for LayoutInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Type: {}", self.name)?;
        writeln!(f, "  size:        {} bytes", self.size)?;
        writeln!(f, "  align:       {} bytes", self.align)?;
        writeln!(f, "  cache lines: {} (64B)", self.cache_lines)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_u64() {
        let info = inspect::<u64>("u64");
        assert_eq!(info.size, 8);
        assert_eq!(info.align, 8);
        assert_eq!(info.cache_lines, 1);
    }

    #[test]
    fn inspect_large_type() {
        let info = inspect::<[u8; 128]>("[u8; 128]");
        assert_eq!(info.size, 128);
        assert_eq!(info.cache_lines, 2);
    }
}
```

- [ ] **Step 5: Create `crates/layout/src/main.rs`**

```rust
//! CLI entry point for struct layout inspection.

use std::io::Write;

#[allow(clippy::print_stdout)]
fn main() {
    use mantis_layout::inspect;
    use mantis_types::{QueueError, SeqNum, SlotIndex};

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "=== Mantis Layout Report ===\n").ok();
    writeln!(out, "{}", inspect::<QueueError>("QueueError")).ok();
    writeln!(out, "{}", inspect::<SeqNum>("SeqNum")).ok();
    writeln!(out, "{}", inspect::<SlotIndex>("SlotIndex")).ok();
}
```

- [ ] **Step 6: Create `crates/verify/Cargo.toml`**

```toml
[package]
name = "mantis-verify"
description = "Formal verification and property testing for the Mantis SDK"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
mantis-core = { workspace = true }
mantis-types = { workspace = true }
mantis-queue = { workspace = true }
bolero = { workspace = true }

[lints]
workspace = true
```

- [ ] **Step 7: Create `crates/verify/src/lib.rs`**

```rust
//! Formal verification and property-based testing for the Mantis SDK.
//!
//! Contains:
//! - Kani proof harnesses for bounded model checking
//! - Bolero property tests for fuzz + property testing
//! - Differential testing utilities
//!
//! This is a `std`-only tooling crate.

#![deny(unsafe_code)]

/// Placeholder for verification utilities.
/// Actual proof harnesses are added alongside the primitives they verify.
pub fn placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_crate_compiles() {
        super::placeholder();
    }
}
```

- [ ] **Step 8: Verify entire workspace compiles**

Run: `cargo check --workspace`
Expected: All 6 crates compile.

- [ ] **Step 9: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass (mantis-types: 4, mantis-bench: 2, mantis-layout: 2, mantis-verify: 1).

- [ ] **Step 10: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: No warnings or errors.

- [ ] **Step 11: Commit**

```bash
git add crates/bench/ crates/layout/ crates/verify/
git commit -m "$(cat <<'EOF'
feat: add mantis-bench, mantis-layout, mantis-verify crate skeletons

mantis-bench: benchmark harness with BenchReport and system detection.
mantis-layout: struct layout inspector with cache-line analysis + CLI.
mantis-verify: placeholder for kani proofs and bolero property tests.
All std-only tooling crates.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Documentation — CLAUDE.md + UNSAFE.md + README.md

**Files:**
- Create: `CLAUDE.md`
- Create: `docs/UNSAFE.md`
- Modify: `README.md`

- [ ] **Step 1: Create `CLAUDE.md`**

```markdown
# Mantis — Low-Latency Financial SDK

## Quick Reference

```
Build:          cargo build --all-features
Test:           cargo test --all-features
Test no_std:    cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features
Lint:           cargo clippy --all-targets --all-features -- -D warnings
Format:         cargo fmt --all
Format check:   cargo fmt --all --check
Deny:           cargo deny check
Miri:           cargo +nightly miri test -p mantis-queue
Careful:        cargo +nightly careful test
Bench:          cargo bench
Bench + ext:    cargo bench --features bench-contenders
Fuzz:           cargo +nightly fuzz run <target>
Layout:         cargo run -p mantis-layout
Kani:           cargo kani -p mantis-verify
Coverage:       cargo llvm-cov --all-features
```

## Architecture

See `philosophy/fin_sdk_oss_blueprint.md` for full SDK vision.
See `philosophy/benchmark_tooling_modular_strategy_design.md` for container strategy.
See `.claude/memory/constantine-reference.md` for reference architecture patterns.

## Workspace Layout

```
crates/core/    mantis-core    Traits, strategy definitions           (no_std)
crates/types/   mantis-types   IDs, newtypes, error types             (no_std)
crates/queue/   mantis-queue   SPSC ring + queue primitives           (no_std)
crates/bench/   mantis-bench   Criterion + custom perf harness        (std)
crates/layout/  mantis-layout  Struct layout / cache-line inspector   (std)
crates/verify/  mantis-verify  Kani proofs, bolero property tests     (std)
```

## no_std Rules

- core, types, queue: `#![no_std]` by default, optional `std` feature
- No heap allocation in hot paths after init
- No panics in hot paths — use `Result` or error enum returns
- bench, layout, verify: `std`-only, never depended on by core crates

## Unsafe Policy

See `docs/UNSAFE.md` for full policy. Summary:
- All unsafe code in `raw` submodules only
- Crate roots: `#![deny(unsafe_code)]`
- Every `unsafe` block: `// SAFETY:` comment (invariant + guarantee + failure mode)
- Miri on every PR, kani proofs nightly

## Code Style

- Newtypes over primitives (`SeqNum(u64)` not raw `u64`)
- Explicit `for` loops over iterator chains in hot paths
- `let...else` for early returns, keep happy path unindented
- No wildcard matches — explicit destructuring
- `#[repr(C)]` + `#[repr(align(...))]` on hot-path structs — document layout
- `#[inline(always)]` only on measured hot functions, never speculatively

## Modular Strategy Pattern

Each primitive has:
1. **Semantic contract** — traits defining behavior guarantees
2. **Strategy traits** — variation points (index, publish, padding, instrumentation)
3. **Generic engine** — internal type parameterized by strategies
4. **Preset aliases** — curated public types (`SpscRingPortable`, `SpscRingPadded`)

## Platform Specialization (Constantine Model)

- Portable baseline always available as reference + fallback
- `cfg(target_arch)` for platform-specific paths — no runtime dispatch in hot paths
- Assembly in dedicated `assembly/` submodules
- All platform variants differential-tested against portable baseline

## Naming Conventions

| Pattern | Convention | Example |
|---|---|---|
| Default (constant-time) | no suffix | `push`, `pop` |
| Variable-time | `_vartime` suffix | `push_vartime` |
| Platform-specific | `_x86_64` / `_aarch64` | `store_head_x86_64` |
| Unsafe internals | in `raw` module | `raw::slot::write_unchecked` |
| Public presets | descriptive | `SpscRingPortable` |

## Priority Order

```
1. Correctness (kani, miri, differential tests)
2. Safety (unsafe isolated, SAFETY comments, no UB)
3. Performance (benchmarked, layout-inspected, asm-verified)
4. Code size / stack usage
5. Ergonomics
```

## Benchmarking

- Never claim "fastest" without published benchmark protocol
- All benchmarks export JSON for cross-hardware comparison
- External contenders behind `bench-contenders` feature flag
- Same workload shapes across all implementations for fair comparison

## Commits

- Imperative mood, ≤72 char subject, one logical change per commit
- Run fmt + clippy + test before committing
- Feature branches, never push directly to main
```

- [ ] **Step 2: Create `docs/UNSAFE.md`**

```markdown
# Unsafe Policy

## Rules

1. All unsafe code lives in `raw` submodules only.
2. Crate roots declare `#![deny(unsafe_code)]`.
3. Only `raw/mod.rs` declares `#![allow(unsafe_code)]`.
4. Every `unsafe` block has a `// SAFETY:` comment that states:
   - Which invariant makes this safe
   - What the caller must guarantee
   - What could go wrong if the invariant is violated
5. Every unsafe function documents preconditions in rustdoc.

## Verification Tiers

| Tier | Method | Runs |
|---|---|---|
| 1 | Unit tests exercising safe API boundaries | Every PR |
| 2 | Miri (stacked borrows + tree borrows) | Every PR |
| 3 | cargo careful (stdlib debug assertions) | Every PR |
| 4 | Kani bounded model checking | Nightly |
| 5 | Differential testing across implementations | Every PR |
| 6 | cargo-mutants (mutation testing) | Nightly |

## Allowed Unsafe Patterns

- `core::sync::atomic` operations with documented ordering rationale
- `MaybeUninit` for uninitialized slot storage in ring buffers
- `core::arch::asm!` for platform-specific fast paths
- `#[repr(C)]` / `#[repr(align)]` casts for layout-controlled types
- `UnsafeCell` for interior mutability in single-writer structures

## Forbidden

- Raw pointer arithmetic when slice indexing works
- `transmute` — use `from_bytes` / `to_bytes` or specific safe casts
- `unsafe impl Send/Sync` without kani proof or formal argument
- Unsafe in tests (use safe API to test unsafe internals)
```

- [ ] **Step 3: Update `README.md`**

```markdown
# Mantis

A modular, `no_std`-first Rust foundation for low-latency financial systems across centralized and decentralized markets, with first-class replay, verification, and performance tooling.

## Status

Phase 0 — Infrastructure bootstrap. Building the SPSC ring and core primitives.

## Quick Start

```bash
# Install tooling
just setup

# Build
cargo build --all-features

# Test
cargo test --all-features

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Benchmark
cargo bench

# Layout inspection
cargo run -p mantis-layout
```

## Architecture

See [CLAUDE.md](CLAUDE.md) for the full development guide.

## License

Apache-2.0
```

- [ ] **Step 4: Verify CLAUDE.md is picked up**

Run: `cat CLAUDE.md | head -3`
Expected: `# Mantis — Low-Latency Financial SDK`

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md docs/UNSAFE.md README.md
git commit -m "$(cat <<'EOF'
docs: add CLAUDE.md, UNSAFE.md, and update README

CLAUDE.md: agent-friendly guide with all commands, architecture,
coding model, strategy pattern, and naming conventions.
UNSAFE.md: unsafe code policy with 6-tier verification.
README: project overview with quick start.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: justfile — Task Runner

**Files:**
- Create: `justfile`

- [ ] **Step 1: Create `justfile`**

```just
# Mantis — Task Runner
# Run `just --list` to see all available commands.

# Install all development tools
setup:
    cargo install cargo-deny --locked
    cargo install cargo-careful --locked
    cargo install cargo-mutants --locked
    cargo install cargo-llvm-cov --locked
    cargo install cargo-fuzz --locked
    cargo install cargo-criterion --locked
    cargo install just --locked
    @echo "Note: kani requires separate install — see docs/specs/"
    @echo "All tools installed."

# Verify all tools are available
check-tools:
    @echo "Checking tools..."
    cargo deny --version
    cargo careful --version
    cargo mutants --version
    cargo llvm-cov --version
    @echo "All tools available."

# Format all code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all --check

# Lint all code
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --all-features

# Run no_std tests
test-no-std:
    cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features

# Run miri on unsafe-containing crates
miri:
    cargo +nightly miri test -p mantis-queue

# Run cargo careful
careful:
    cargo +nightly careful test

# Run cargo deny
deny:
    cargo deny check

# Build docs
doc:
    cargo doc --no-deps --all-features --open

# Run all CI checks locally
ci: fmt-check lint test test-no-std deny doc

# Run benchmarks
bench:
    cargo bench

# Run layout inspector
layout:
    cargo run -p mantis-layout

# Coverage report
coverage:
    cargo llvm-cov --all-features --html
    @echo "Report: target/llvm-cov/html/index.html"

# Coverage with branch coverage
coverage-branch:
    cargo llvm-cov --all-features --branch --html
    @echo "Report: target/llvm-cov/html/index.html"
```

- [ ] **Step 2: Verify justfile works**

Run: `just --list`
Expected: All commands listed.

- [ ] **Step 3: Run `just ci` (minus miri/careful — those need nightly)**

Run: `just fmt-check && just lint && just test && just deny`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add justfile
git commit -m "$(cat <<'EOF'
chore: add justfile task runner

Commands for setup, lint, test, bench, coverage, miri, careful,
deny, and a local CI check that mirrors the PR gate.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: CI — PR Gate (`ci.yml`)

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -Dwarnings

jobs:
  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all --check

  clippy:
    name: Clippy
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --all-targets --all-features -- -D warnings

  test:
    name: Test (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all-features

  test-no-std:
    name: Test no_std (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features

  doc:
    name: Documentation
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo doc --no-deps --all-features
        env:
          RUSTDOCFLAGS: -Dwarnings

  deny:
    name: Cargo Deny
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/install-action@cargo-deny
      - run: cargo deny check

  miri:
    name: Miri
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: miri
      - uses: Swatinem/rust-cache@v2
      - run: cargo miri test -p mantis-queue --all-features
        env:
          MIRIFLAGS: -Zmiri-strict-provenance -Zmiri-symbolic-alignment-check

  careful:
    name: Cargo Careful
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: taiki-e/install-action@cargo-careful
      - uses: Swatinem/rust-cache@v2
      - run: cargo careful test --all-features

  coverage:
    name: Coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: taiki-e/install-action@cargo-llvm-cov
      - uses: Swatinem/rust-cache@v2
      - name: Line coverage (codecov format)
        run: cargo llvm-cov --all-features --codecov --output-path codecov.json
      - name: Branch coverage
        run: cargo llvm-cov --all-features --branch --lcov --output-path lcov-branch.info
      - name: Upload coverage
        if: github.event_name == 'pull_request'
        uses: actions/upload-artifact@v4
        with:
          name: coverage-report
          path: |
            codecov.json
            lcov-branch.info
```

- [ ] **Step 2: Verify YAML is valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo "Valid YAML"`
Expected: `Valid YAML`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci: add PR gate workflow

Runs on every PR and push to main:
fmt, clippy, test, test-no-std, doc, deny, miri, careful, coverage.
Cross-platform: Linux x86_64 + macOS ARM64.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: CI — Benchmark Regression (`bench.yml`)

**Files:**
- Create: `.github/workflows/bench.yml`

- [ ] **Step 1: Create `.github/workflows/bench.yml`**

```yaml
name: Benchmarks

on:
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  benchmark:
    name: Benchmark Regression
    runs-on: ubuntu-latest
    steps:
      - name: Checkout PR
        uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Run benchmarks
        run: cargo bench --all-features -- --output-format bencher | tee bench-output.txt

      - name: Upload benchmark results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: bench-output.txt

      # Note: benchmark comparison against baseline will be configured
      # once there are actual benchmarks to compare. The infrastructure
      # is ready — criterion generates baselines automatically.
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/bench.yml
git commit -m "$(cat <<'EOF'
ci: add benchmark regression workflow

Runs criterion benchmarks on every PR. Uploads results as artifacts.
Comparison logic activates when actual benchmarks exist.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: CI — Nightly Extended Checks (`nightly.yml`)

**Files:**
- Create: `.github/workflows/nightly.yml`

- [ ] **Step 1: Create `.github/workflows/nightly.yml`**

```yaml
name: Nightly

on:
  schedule:
    - cron: "0 3 * * *"  # 3 AM UTC daily
  workflow_dispatch:

env:
  CARGO_TERM_COLOR: always

jobs:
  mutants:
    name: Mutation Testing
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: taiki-e/install-action@cargo-mutants
      - uses: Swatinem/rust-cache@v2
      - run: cargo mutants --timeout 60 -- --all-features
        continue-on-error: true
      - name: Upload mutants report
        uses: actions/upload-artifact@v4
        with:
          name: mutants-report
          path: mutants.out/

  miri-extended:
    name: Miri Extended
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: miri
      - uses: Swatinem/rust-cache@v2
      - name: Miri with stacked borrows
        run: cargo miri test -p mantis-queue --all-features
        env:
          MIRIFLAGS: -Zmiri-strict-provenance -Zmiri-symbolic-alignment-check
      - name: Miri with tree borrows
        run: cargo miri test -p mantis-queue --all-features
        env:
          MIRIFLAGS: -Zmiri-tree-borrows -Zmiri-strict-provenance

  coverage-full:
    name: Full Coverage Report
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: taiki-e/install-action@cargo-llvm-cov
      - uses: Swatinem/rust-cache@v2
      - run: cargo llvm-cov --all-features --branch --html
      - name: Upload coverage report
        uses: actions/upload-artifact@v4
        with:
          name: coverage-html
          path: target/llvm-cov/html/

  asm-check:
    name: ASM Toggle (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Test with default features
        run: cargo test --all-features
      - name: Test with no default features
        run: cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/nightly.yml
git commit -m "$(cat <<'EOF'
ci: add nightly extended checks workflow

Scheduled daily: mutation testing, extended miri (stacked + tree
borrows), full coverage report, ASM toggle checks. All results
uploaded as artifacts.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: CI — Formal Verification (`verify.yml`) + Dependabot

**Files:**
- Create: `.github/workflows/verify.yml`
- Create: `.github/dependabot.yml`

- [ ] **Step 1: Create `.github/workflows/verify.yml`**

```yaml
name: Verification

on:
  schedule:
    - cron: "0 4 * * *"  # 4 AM UTC daily
  workflow_dispatch:

env:
  CARGO_TERM_COLOR: always

jobs:
  kani:
    name: Kani Proofs
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Kani
        uses: model-checking/kani-verifier-action@v1
      - name: Run proofs
        run: |
          if find crates/verify/src -name '*.rs' -exec grep -l 'kani::proof' {} + 2>/dev/null; then
            cargo kani -p mantis-verify
          else
            echo "No kani proofs found yet — skipping."
          fi

  bolero:
    name: Bolero Property Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run property tests
        run: cargo test -p mantis-verify --all-features -- --test-threads=1
```

- [ ] **Step 2: Create `.github/dependabot.yml`**

```yaml
version: 2
updates:
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 10
    groups:
      minor-and-patch:
        update-types:
          - minor
          - patch

  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
    open-pull-requests-limit: 5
    groups:
      actions:
        patterns:
          - "*"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/verify.yml .github/dependabot.yml
git commit -m "$(cat <<'EOF'
ci: add formal verification workflow and dependabot config

verify.yml: nightly kani proofs and bolero property tests.
dependabot.yml: weekly cargo + actions updates with grouped PRs.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: Final Validation — Full CI Check Locally

**Files:** None (validation only)

- [ ] **Step 1: Format check**

Run: `cargo fmt --all --check`
Expected: No formatting issues.

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Full test suite**

Run: `cargo test --all-features`
Expected: All tests pass (9 tests across workspace).

- [ ] **Step 4: no_std test**

Run: `cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features`
Expected: Compiles and tests pass.

- [ ] **Step 5: Cargo deny**

Run: `cargo deny check`
Expected: No issues (or only advisories for transitive deps).

- [ ] **Step 6: Docs build**

Run: `cargo doc --no-deps --all-features`
Expected: Docs build without warnings.

- [ ] **Step 7: Layout inspector works**

Run: `cargo run -p mantis-layout`
Expected: Prints layout report for QueueError, SeqNum, SlotIndex.

- [ ] **Step 8: Verify git status is clean**

Run: `git status`
Expected: Working tree clean, all changes committed.

- [ ] **Step 9: Summary**

At this point the bootstrap is complete:
- 6 crates compile and pass tests
- `CLAUDE.md` guides agents
- `UNSAFE.md` documents safety policy
- `justfile` provides all commands
- 4 CI workflows ready for GitHub
- `deny.toml` guards supply chain
- Clippy pedantic + strict lints enforced
- The workspace is ready for SPSC ring implementation
