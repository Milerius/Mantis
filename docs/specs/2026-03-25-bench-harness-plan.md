# Benchmark Harness — Implementation Plan (2 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the benchmark harness in `mantis-bench` with platform cycle counters, criterion `Measurement` integration, standardized workload shapes, external contender benchmarks, and a Godbolt ASM inspection script.

**Architecture:** Platform-aware cycle counters selected at compile time via `cfg`. A generic `MantisMeasurement<C>` implements criterion's `Measurement` trait. Workload shapes are reusable functions applied identically to all implementations. C++ contenders are compiled via `cc` crate and called through FFI.

**Tech Stack:** Rust std, criterion 0.5, serde/serde_json, `core::arch::asm!` (x86_64 RDTSC), `cc` crate (C++ FFI), shell/curl (Godbolt API).

**Spec:** `docs/specs/2026-03-25-spsc-ring-bench-design.md` — Sections 3 (Benchmark Harness) and 5.2 (ASM toggle for bench).

**Prerequisite:** Plan 1 (SPSC Ring Buffer) must be complete — benchmarks depend on `mantis-queue` presets.

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `crates/bench/src/counters/mod.rs` | Re-export counter types, `DefaultCounter` type alias |
| `crates/bench/src/counters/rdtsc.rs` | `RdtscCounter` — x86_64 `lfence; rdtsc; lfence` (behind `asm` feature) |
| `crates/bench/src/counters/kperf.rs` | `KperfCounter` — macOS ARM64 `mach_absolute_time` |
| `crates/bench/src/counters/pmu.rs` | `PmuCounter` — Linux ARM64 `clock_gettime` |
| `crates/bench/src/measurement.rs` | `MantisMeasurement<C>` implementing criterion `Measurement` trait |
| `crates/bench/src/workloads.rs` | Workload shape functions: `single_item`, `burst`, `ping_pong`, `full_drain` |
| `crates/bench/benches/spsc_mantis.rs` | Criterion benchmarks for all Mantis SPSC presets |
| `crates/bench/benches/spsc_contenders.rs` | Criterion benchmarks for rtrb, crossbeam (behind `bench-contenders`) |
| `scripts/check-asm.sh` | Godbolt API ASM inspection script |

### Modified files

| File | Changes |
|---|---|
| `crates/bench/Cargo.toml` | Add `asm`, `bench-contenders` features; add deps (`cc`, `rtrb`, `crossbeam-queue`) |
| `crates/bench/src/lib.rs` | Add modules, re-exports |
| `crates/bench/src/counters.rs` | Convert to `crates/bench/src/counters/mod.rs` (move existing code) |
| `crates/bench/src/report.rs` | Extend `BenchReport` with compiler, features, results |

---

## Task 1: Restructure counters into sub-module directory

**Files:**
- Delete: `crates/bench/src/counters.rs`
- Create: `crates/bench/src/counters/mod.rs`

- [ ] **Step 1: Move existing `counters.rs` content to `counters/mod.rs`**

Create `crates/bench/src/counters/mod.rs` with the exact content of the current `counters.rs`, plus sub-module declarations:

```rust
//! Platform-aware performance counter collection.
//!
//! Counter selection is compile-time via `cfg(target_arch)` +
//! `cfg(target_os)`. No runtime dispatch.

mod instant;

#[cfg(all(target_arch = "x86_64", feature = "asm"))]
mod rdtsc;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod kperf;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod pmu;

pub use instant::InstantCounter;

#[cfg(all(target_arch = "x86_64", feature = "asm"))]
pub use rdtsc::RdtscCounter;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use kperf::KperfCounter;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub use pmu::PmuCounter;

/// A measurement from a performance counter.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Wall-clock duration in nanoseconds.
    pub nanos: u64,
    /// CPU cycles (if available on this platform, else 0).
    pub cycles: u64,
}

/// Trait for platform-specific cycle counting.
pub trait CycleCounter: Send + Sync {
    /// Start a measurement.
    fn start(&self) -> u64;
    /// End a measurement, returning elapsed.
    fn elapsed(&self, start: u64) -> Measurement;
}

/// Compile-time selected default counter for the current platform.
#[cfg(all(target_arch = "x86_64", feature = "asm"))]
pub type DefaultCounter = RdtscCounter;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub type DefaultCounter = KperfCounter;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub type DefaultCounter = PmuCounter;

#[cfg(not(any(
    all(target_arch = "x86_64", feature = "asm"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "aarch64"),
)))]
pub type DefaultCounter = InstantCounter;
```

- [ ] **Step 2: Create `counters/instant.rs`**

Move `InstantCounter` into its own file:

```rust
//! Fallback counter using `std::time::Instant`.

use std::time::Instant;

use super::{CycleCounter, Measurement};

/// Fallback counter using `std::time::Instant`.
pub struct InstantCounter {
    epoch: Instant,
}

impl InstantCounter {
    /// Create a new counter with current time as epoch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for InstantCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for InstantCounter {
    fn start(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos())
            .unwrap_or(u64::MAX)
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let now = u64::try_from(self.epoch.elapsed().as_nanos())
            .unwrap_or(u64::MAX);
        Measurement {
            nanos: now.saturating_sub(start),
            cycles: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instant_counter_measures_time() {
        let counter = InstantCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..1000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert_eq!(m.cycles, 0, "fallback counter has no cycle info");
    }
}
```

- [ ] **Step 3: Delete old `counters.rs` file**

Remove `crates/bench/src/counters.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-bench`
Expected: PASS — existing tests still work

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/counters/
git rm crates/bench/src/counters.rs
git commit -m "refactor(bench): restructure counters into sub-module directory"
```

---

## Task 2: RDTSC cycle counter (x86_64, `asm` feature)

**Files:**
- Create: `crates/bench/src/counters/rdtsc.rs`
- Modify: `crates/bench/Cargo.toml` (add `asm` feature)

- [ ] **Step 1: Add `asm` feature to `Cargo.toml`**

```toml
[features]
default = []
asm = []
bench-contenders = ["dep:rtrb", "dep:crossbeam-queue"]
```

- [ ] **Step 2: Write test**

In `crates/bench/src/counters/rdtsc.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdtsc_measures_cycles() {
        let counter = RdtscCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..10_000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert!(m.cycles > 0, "RDTSC should report non-zero cycles");
        assert!(m.nanos > 0, "should also report nanos");
    }
}
```

- [ ] **Step 3: Implement `RdtscCounter`**

Note: `mantis-bench` has `#![deny(unsafe_code)]` at crate root. The `rdtsc.rs` file needs `#![allow(unsafe_code)]` because inline asm is inherently unsafe. This is acceptable for the bench crate (std-only tooling, not hot-path production code) — the `raw` submodule rule in UNSAFE.md applies to `mantis-queue`/`mantis-core`. The implementer should add a comment explaining this exception.

```rust
//! x86_64 RDTSC cycle counter with lfence serialization.
//!
//! Following the Constantine model:
//! - `lfence` before `rdtsc` serializes (prevents out-of-order reads)
//! - `lfence` after prevents speculative reads past measurement point
//!
//! # Unsafe code
//!
//! This file allows unsafe for inline asm (RDTSC). The `raw` submodule
//! policy in UNSAFE.md applies to core/queue crates; the bench crate's
//! cycle counters inherently require platform-specific unsafe.

#![allow(unsafe_code)]

use std::time::Instant;

use super::{CycleCounter, Measurement};

/// RDTSC-based cycle counter for x86_64.
///
/// Uses `lfence; rdtsc; lfence` for accurate serialized cycle reads.
pub struct RdtscCounter {
    epoch: Instant,
}

impl RdtscCounter {
    /// Create a new RDTSC counter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }

    /// Read the TSC with lfence serialization.
    #[inline]
    fn rdtsc_serialized() -> u64 {
        let lo: u32;
        let hi: u32;
        // SAFETY: lfence + rdtsc is a valid instruction sequence on
        // all x86_64 CPUs. It reads the timestamp counter without
        // side effects. The lfence before serializes prior loads;
        // the lfence after prevents speculative loads past rdtsc.
        unsafe {
            core::arch::asm!(
                "lfence",
                "rdtsc",
                "lfence",
                out("eax") lo,
                out("edx") hi,
                options(nostack, nomem, preserves_flags),
            );
        }
        u64::from(hi) << 32 | u64::from(lo)
    }
}

impl Default for RdtscCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for RdtscCounter {
    #[inline]
    fn start(&self) -> u64 {
        Self::rdtsc_serialized()
    }

    #[inline]
    fn elapsed(&self, start: u64) -> Measurement {
        let end = Self::rdtsc_serialized();
        Measurement {
            nanos: u64::try_from(self.epoch.elapsed().as_nanos())
                .unwrap_or(u64::MAX),
            cycles: end.saturating_sub(start),
        }
    }
}
```

- [ ] **Step 4: Run tests (x86_64 only)**

Run: `cargo test -p mantis-bench --features asm`
Expected: PASS on x86_64 machines; skipped on ARM

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/counters/rdtsc.rs crates/bench/Cargo.toml
git commit -m "feat(bench): add RdtscCounter with lfence serialization"
```

---

## Task 3: macOS ARM64 counter (`KperfCounter`)

**Files:**
- Create: `crates/bench/src/counters/kperf.rs`

- [ ] **Step 1: Write test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kperf_measures_nanos() {
        let counter = KperfCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..10_000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert!(m.nanos > 0, "should measure non-zero nanos");
    }
}
```

- [ ] **Step 2: Implement `KperfCounter`**

Note: Same unsafe exception as `rdtsc.rs` — FFI calls to macOS system APIs are inherently unsafe. See rdtsc.rs note for justification.

```rust
//! macOS ARM64 counter using `mach_absolute_time`.
//!
//! # Unsafe code
//!
//! This file allows unsafe for FFI to macOS Mach APIs.
//! See `rdtsc.rs` for the bench crate unsafe policy justification.

#![allow(unsafe_code)]

use super::{CycleCounter, Measurement};

extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

/// macOS ARM64 high-resolution counter.
///
/// Uses `mach_absolute_time()` which returns ticks in Mach absolute
/// time units. Converts to nanoseconds via the timebase ratio.
pub struct KperfCounter {
    numer: u64,
    denom: u64,
}

impl KperfCounter {
    /// Create a new kperf counter, querying the timebase info.
    #[must_use]
    pub fn new() -> Self {
        let mut info = MachTimebaseInfo { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info is a stable macOS API that fills
        // the provided struct. It always succeeds (returns 0).
        unsafe {
            mach_timebase_info(&mut info);
        }
        Self {
            numer: u64::from(info.numer),
            denom: u64::from(info.denom),
        }
    }

    fn ticks_to_nanos(&self, ticks: u64) -> u64 {
        ticks.saturating_mul(self.numer) / self.denom
    }
}

impl Default for KperfCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for KperfCounter {
    #[inline]
    fn start(&self) -> u64 {
        // SAFETY: mach_absolute_time is a stable, side-effect-free
        // macOS API that returns a monotonic tick count.
        unsafe { mach_absolute_time() }
    }

    #[inline]
    fn elapsed(&self, start: u64) -> Measurement {
        // SAFETY: Same as start().
        let now = unsafe { mach_absolute_time() };
        let ticks = now.saturating_sub(start);
        Measurement {
            nanos: self.ticks_to_nanos(ticks),
            // On Apple Silicon, mach_absolute_time ticks ~= cycles
            // (1:1 ratio on M1/M2/M3), so we report ticks as cycles.
            cycles: ticks,
        }
    }
}
```

- [ ] **Step 3: Run tests (macOS ARM64 only)**

Run: `cargo test -p mantis-bench`
Expected: PASS on macOS ARM64

- [ ] **Step 4: Commit**

```bash
git add crates/bench/src/counters/kperf.rs
git commit -m "feat(bench): add KperfCounter for macOS ARM64"
```

---

## Task 4: Linux ARM64 counter (`PmuCounter`)

**Files:**
- Create: `crates/bench/src/counters/pmu.rs`

- [ ] **Step 1: Write test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmu_measures_nanos() {
        let counter = PmuCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..10_000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert!(m.nanos > 0, "should measure non-zero nanos");
    }
}
```

- [ ] **Step 2: Implement `PmuCounter`**

Note: Same unsafe exception as `rdtsc.rs` — FFI calls to libc are inherently unsafe. The `libc` dep should be added to the workspace `Cargo.toml` first, then referenced via workspace inheritance. The implementer must look up the current stable version.

```rust
//! Linux ARM64 counter using `clock_gettime(CLOCK_MONOTONIC)`.
//!
//! # Unsafe code
//!
//! This file allows unsafe for FFI to libc clock APIs.
//! See `rdtsc.rs` for the bench crate unsafe policy justification.

#![allow(unsafe_code)]

use super::{CycleCounter, Measurement};

/// Linux ARM64 high-resolution counter.
///
/// Uses `clock_gettime(CLOCK_MONOTONIC)` for nanosecond-precision
/// timing. Cycles are not directly available without perf_event_open
/// (which requires privileges), so cycles = 0 by default.
pub struct PmuCounter;

impl PmuCounter {
    /// Create a new PMU counter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn clock_gettime_nanos() -> u64 {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime with CLOCK_MONOTONIC is always valid
        // on Linux. The timespec struct is correctly sized and aligned.
        unsafe {
            libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        }
        // CLOCK_MONOTONIC guarantees tv_sec >= 0 and tv_nsec in [0, 999_999_999]
        #[allow(clippy::cast_sign_loss)]
        let nanos = (ts.tv_sec as u64)
            .saturating_mul(1_000_000_000)
            .saturating_add(ts.tv_nsec as u64);
        nanos
    }
}

impl Default for PmuCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for PmuCounter {
    #[inline]
    fn start(&self) -> u64 {
        Self::clock_gettime_nanos()
    }

    #[inline]
    fn elapsed(&self, start: u64) -> Measurement {
        let now = Self::clock_gettime_nanos();
        Measurement {
            nanos: now.saturating_sub(start),
            cycles: 0,
        }
    }
}
```

Note: This requires adding `libc` to the workspace `Cargo.toml` first (the implementer must look up the current stable version and pin it exactly), then referencing it in `crates/bench/Cargo.toml`:

```toml
# In workspace Cargo.toml [workspace.dependencies]:
# libc = "0.2.X"  # look up exact latest

# In crates/bench/Cargo.toml:
[target.'cfg(target_os = "linux")'.dependencies]
libc = { workspace = true }
```

- [ ] **Step 3: Run tests (Linux ARM64 only)**

Run: `cargo test -p mantis-bench`
Expected: PASS on Linux ARM64; skipped elsewhere

- [ ] **Step 4: Commit**

```bash
git add crates/bench/src/counters/pmu.rs crates/bench/Cargo.toml
git commit -m "feat(bench): add PmuCounter for Linux ARM64"
```

---

## Task 5: `MantisMeasurement<C>` — criterion integration

**Files:**
- Create: `crates/bench/src/measurement.rs`
- Modify: `crates/bench/src/lib.rs`

- [ ] **Step 1: Write test**

In `crates/bench/src/measurement.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::counters::InstantCounter;

    #[test]
    fn measurement_creation() {
        let m = MantisMeasurement::new(InstantCounter::new());
        // Just verify it compiles and the counter is accessible
        let _start = m.counter.start();
    }
}
```

- [ ] **Step 2: Implement `MantisMeasurement<C>`**

```rust
//! Criterion `Measurement` implementation backed by platform counters.

use std::time::Duration;

use criterion::measurement::{Measurement, ValueFormatter, WallTime};

use crate::counters::{CycleCounter, DefaultCounter};

/// Criterion measurement backed by a platform cycle counter.
///
/// Generic over the counter to avoid runtime dispatch. Use
/// `DefaultMeasurement` for the compile-time selected platform counter.
pub struct MantisMeasurement<C: CycleCounter> {
    pub(crate) counter: C,
    wall: WallTime,
}

impl<C: CycleCounter> MantisMeasurement<C> {
    /// Create a new measurement with the given counter.
    pub fn new(counter: C) -> Self {
        Self {
            counter,
            wall: WallTime,
        }
    }
}

impl<C: CycleCounter + 'static> Measurement for MantisMeasurement<C> {
    type Intermediate = (u64, std::time::Instant);
    type Value = Duration;

    fn start(&self) -> Self::Intermediate {
        let cycles = self.counter.start();
        let wall_start = std::time::Instant::now();
        (cycles, wall_start)
    }

    fn end(&self, i: Self::Intermediate) -> Self::Value {
        let wall_elapsed = i.1.elapsed();
        let _ = self.counter.elapsed(i.0); // Cycles available if needed
        wall_elapsed
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        *v1 + *v2
    }

    fn zero(&self) -> Self::Value {
        Duration::ZERO
    }

    fn to_f64(&self, value: &Self::Value) -> f64 {
        value.as_nanos() as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        self.wall.formatter()
    }
}

/// Default measurement using the platform-selected counter.
pub type DefaultMeasurement = MantisMeasurement<DefaultCounter>;

impl DefaultMeasurement {
    /// Create a default measurement for the current platform.
    pub fn platform_default() -> Self {
        Self::new(DefaultCounter::default())
    }
}
```

Note: The criterion `Measurement` trait integration may need adjustment based on the exact criterion 0.5 API. The implementer should verify against criterion's actual trait definition and adjust types as needed.

- [ ] **Step 3: Wire up in `lib.rs`**

Add to `crates/bench/src/lib.rs`:

```rust
pub mod measurement;
pub mod workloads;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-bench`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/measurement.rs crates/bench/src/lib.rs
git commit -m "feat(bench): add MantisMeasurement criterion integration"
```

---

## Task 6: Extend `BenchReport` with full metadata

**Files:**
- Modify: `crates/bench/src/report.rs`

- [ ] **Step 1: Write tests**

```rust
use super::*;

#[test]
fn report_serializes_to_json() {
    let report = BenchReport {
        implementation: "SpscRing".to_owned(),
        arch: "x86_64".to_owned(),
        os: "linux".to_owned(),
        cpu: "AMD Ryzen 9 7950X".to_owned(),
        compiler: "rustc 1.85.0".to_owned(),
        features: vec!["asm".to_owned()],
        results: vec![WorkloadResult {
            workload: "single_item".to_owned(),
            element_type: "u64".to_owned(),
            ops_per_sec: 100_000_000.0,
            ns_per_op: 10.0,
            cycles_per_op: Some(35.0),
            p50_ns: 9.0,
            p99_ns: 15.0,
            p999_ns: 50.0,
        }],
    };
    let json = serde_json::to_string_pretty(&report)
        .expect("serialization failed");
    assert!(json.contains("SpscRing"));
    assert!(json.contains("single_item"));
}
```

- [ ] **Step 2: Extend `BenchReport`**

Note: The existing `BenchReport::detect()` takes no arguments. This change adds an `implementation: &str` parameter. The existing test `detect_fills_arch_and_os` must be updated to pass a string argument: `BenchReport::detect("test")`.



```rust
//! Benchmark report metadata and serialization.

/// Benchmark report metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchReport {
    /// Name of the implementation being benchmarked.
    pub implementation: String,
    /// Target architecture (e.g., `"x86_64"`, `"aarch64"`).
    pub arch: String,
    /// Operating system.
    pub os: String,
    /// CPU model name.
    pub cpu: String,
    /// Rust compiler version.
    pub compiler: String,
    /// Enabled feature flags.
    pub features: Vec<String>,
    /// Workload results.
    pub results: Vec<WorkloadResult>,
}

/// Result for a single workload shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkloadResult {
    /// Workload name (e.g., `"single_item"`, `"burst_100"`).
    pub workload: String,
    /// Element type (e.g., `"u64"`, `"[u8; 64]"`).
    pub element_type: String,
    /// Operations per second.
    pub ops_per_sec: f64,
    /// Nanoseconds per operation.
    pub ns_per_op: f64,
    /// CPU cycles per operation (if available).
    pub cycles_per_op: Option<f64>,
    /// 50th percentile latency in nanoseconds.
    pub p50_ns: f64,
    /// 99th percentile latency in nanoseconds.
    pub p99_ns: f64,
    /// 99.9th percentile latency in nanoseconds.
    pub p999_ns: f64,
}

impl BenchReport {
    /// Create a new report with detected system info.
    #[must_use]
    pub fn detect(implementation: &str) -> Self {
        Self {
            implementation: implementation.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            os: std::env::consts::OS.to_owned(),
            cpu: detect_cpu_name(),
            compiler: detect_rustc_version(),
            features: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Export the report to JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

fn detect_cpu_name() -> String {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|| "unknown".to_owned())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|n| n.trim().to_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "unknown".to_owned()
    }
}

fn detect_rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p mantis-bench`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/bench/src/report.rs
git commit -m "feat(bench): extend BenchReport with compiler, features, results"
```

---

## Task 7: Workload shapes

**Files:**
- Create: `crates/bench/src/workloads.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mantis_queue::SpscRing;

    #[test]
    fn single_item_workload() {
        let mut ring = SpscRing::<u64, 64>::new();
        single_item(&mut ring, 100);
    }

    #[test]
    fn burst_workload() {
        let mut ring = SpscRing::<u64, 128>::new();
        burst(&mut ring, 50, 100);
    }

    #[test]
    fn full_drain_workload() {
        let mut ring = SpscRing::<u64, 64>::new();
        full_drain(&mut ring, 10);
    }
}
```

- [ ] **Step 2: Implement workload shapes**

```rust
//! Standardized workload shapes for SPSC ring benchmarks.
//!
//! All workloads operate on a `RawRing`-like interface (try_push/try_pop).
//! They are used identically across all implementations for fair comparison.

use mantis_queue::RawRing;
use mantis_queue::storage::Storage;
use mantis_core::{IndexStrategy, Instrumentation, PushPolicy};
use mantis_types::PushError;

/// Push 1, pop 1, repeat `n` times. Measures per-op latency.
pub fn single_item<T, S, I, P, Instr>(
    ring: &mut RawRing<T, S, I, P, Instr>,
    n: usize,
) where
    T: From<u64> + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    for i in 0..n {
        let val = T::from(i as u64);
        let _ = ring.try_push(val);
        let _ = ring.try_pop();
    }
}

/// Push `burst_size` items, then pop all, repeat `rounds` times.
pub fn burst<T, S, I, P, Instr>(
    ring: &mut RawRing<T, S, I, P, Instr>,
    burst_size: usize,
    rounds: usize,
) where
    T: From<u64> + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    for round in 0..rounds {
        for i in 0..burst_size {
            let val = T::from((round * burst_size + i) as u64);
            if ring.try_push(val).is_err() {
                break;
            }
        }
        while ring.try_pop().is_ok() {}
    }
}

/// Fill ring completely, drain completely, repeat `rounds` times.
pub fn full_drain<T, S, I, P, Instr>(
    ring: &mut RawRing<T, S, I, P, Instr>,
    rounds: usize,
) where
    T: From<u64> + Send,
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    let cap = ring.capacity();
    for round in 0..rounds {
        for i in 0..cap {
            let val = T::from((round * cap + i) as u64);
            let _ = ring.try_push(val);
        }
        while ring.try_pop().is_ok() {}
    }
}
```

Note: The `ping_pong` workload (two-thread) is more complex and should be implemented as a criterion benchmark function directly in `benches/spsc_mantis.rs` rather than a generic function, since it requires thread spawning and synchronization.

- [ ] **Step 3: Add `mantis-queue` dependency to bench Cargo.toml**

Ensure `crates/bench/Cargo.toml` has:

```toml
[dependencies]
mantis-queue = { workspace = true, features = ["alloc"] }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-bench --all-features`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/bench/src/workloads.rs crates/bench/Cargo.toml
git commit -m "feat(bench): add standardized workload shapes"
```

---

## Task 8: Criterion benchmarks for Mantis presets

**Files:**
- Create: `crates/bench/benches/spsc_mantis.rs`
- Modify: `crates/bench/Cargo.toml` (add `[[bench]]` section)

- [ ] **Step 1: Add bench target to Cargo.toml**

```toml
[[bench]]
name = "spsc_mantis"
harness = false

[[bench]]
name = "spsc_contenders"
harness = false
required-features = ["bench-contenders"]
```

- [ ] **Step 2: Write the benchmark**

In `crates/bench/benches/spsc_mantis.rs`:

```rust
//! Criterion benchmarks for all Mantis SPSC ring presets.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use mantis_queue::SpscRing;

#[cfg(feature = "alloc")]
use mantis_queue::SpscRingHeap;

fn bench_single_item(c: &mut Criterion) {
    c.bench_function("spsc/inline/single_item/u64", |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box(42u64));
            let _ = black_box(ring.try_pop());
        });
    });
}

fn bench_burst_100(c: &mut Criterion) {
    c.bench_function("spsc/inline/burst_100/u64", |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            for i in 0..100u64 {
                let _ = ring.try_push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(ring.try_pop());
            }
        });
    });
}

fn bench_full_drain(c: &mut Criterion) {
    c.bench_function("spsc/inline/full_drain/u64", |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            for i in 0..1023u64 {
                let _ = ring.try_push(black_box(i));
            }
            while ring.try_pop().is_ok() {}
        });
    });
}

fn bench_burst_1000(c: &mut Criterion) {
    c.bench_function("spsc/inline/burst_1000/u64", |b| {
        let mut ring = SpscRing::<u64, 2048>::new();
        b.iter(|| {
            for i in 0..1000u64 {
                let _ = ring.try_push(black_box(i));
            }
            for _ in 0..1000 {
                let _ = black_box(ring.try_pop());
            }
        });
    });
}

fn bench_single_item_64b(c: &mut Criterion) {
    c.bench_function("spsc/inline/single_item/[u8;64]", |b| {
        let mut ring = SpscRing::<[u8; 64], 1024>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box([0u8; 64]));
            let _ = black_box(ring.try_pop());
        });
    });
}

fn bench_single_item_256b(c: &mut Criterion) {
    c.bench_function("spsc/inline/single_item/[u8;256]", |b| {
        let mut ring = SpscRing::<[u8; 256], 256>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box([0u8; 256]));
            let _ = black_box(ring.try_pop());
        });
    });
}

criterion_group!(
    benches,
    bench_single_item,
    bench_burst_100,
    bench_burst_1000,
    bench_full_drain,
    bench_single_item_64b,
    bench_single_item_256b,
);
criterion_main!(benches);
```

- [ ] **Step 3: Verify benchmarks compile and run**

Run: `cargo bench -p mantis-bench --bench spsc_mantis -- --quick`
Expected: Benchmark output with timing data

- [ ] **Step 4: Commit**

```bash
git add crates/bench/benches/spsc_mantis.rs crates/bench/Cargo.toml
git commit -m "feat(bench): add criterion benchmarks for Mantis SPSC presets"
```

---

## Task 9: External contender benchmarks (Rust only — rtrb, crossbeam)

**Files:**
- Modify: `crates/bench/Cargo.toml` (add optional deps)
- Create: `crates/bench/benches/spsc_contenders.rs`

- [ ] **Step 1: Add optional dependencies**

In `crates/bench/Cargo.toml`:

```toml
[dependencies]
# ... existing ...
rtrb = { version = "0.3", optional = true }
crossbeam-queue = { version = "0.3", optional = true }
```

- [ ] **Step 2: Write contender benchmarks**

In `crates/bench/benches/spsc_contenders.rs`:

```rust
//! Criterion benchmarks for external SPSC ring contenders.
//!
//! Requires `bench-contenders` feature flag.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_rtrb_single_item(c: &mut Criterion) {
    c.bench_function("spsc/rtrb/single_item/u64", |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        b.iter(|| {
            let _ = tx.push(black_box(42u64));
            let _ = black_box(rx.pop());
        });
    });
}

fn bench_crossbeam_single_item(c: &mut Criterion) {
    c.bench_function("spsc/crossbeam/single_item/u64", |b| {
        let q = crossbeam_queue::ArrayQueue::new(1024);
        b.iter(|| {
            let _ = q.push(black_box(42u64));
            let _ = black_box(q.pop());
        });
    });
}

fn bench_rtrb_burst(c: &mut Criterion) {
    c.bench_function("spsc/rtrb/burst_100/u64", |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        b.iter(|| {
            for i in 0..100u64 {
                let _ = tx.push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(rx.pop());
            }
        });
    });
}

fn bench_crossbeam_burst(c: &mut Criterion) {
    c.bench_function("spsc/crossbeam/burst_100/u64", |b| {
        let q = crossbeam_queue::ArrayQueue::new(1024);
        b.iter(|| {
            for i in 0..100u64 {
                let _ = q.push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(q.pop());
            }
        });
    });
}

criterion_group!(
    contenders,
    bench_rtrb_single_item,
    bench_crossbeam_single_item,
    bench_rtrb_burst,
    bench_crossbeam_burst,
);
criterion_main!(contenders);
```

- [ ] **Step 3: Verify contender benchmarks compile**

Run: `cargo bench -p mantis-bench --bench spsc_contenders --features bench-contenders -- --quick`
Expected: Benchmark output

- [ ] **Step 4: Commit**

```bash
git add crates/bench/benches/spsc_contenders.rs crates/bench/Cargo.toml
git commit -m "feat(bench): add rtrb and crossbeam contender benchmarks"
```

---

## Task 10: Godbolt ASM inspection script

**Files:**
- Create: `scripts/check-asm.sh`

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Godbolt ASM inspection for Mantis hot functions.
#
# Usage: ./scripts/check-asm.sh [--baseline]
#
# Sends push/pop implementations to Godbolt Compiler Explorer API
# for x86_64 and aarch64, saves output to target/asm/.
# With --baseline, saves to target/asm/baseline/ for future diffs.

GODBOLT_API="https://godbolt.org/api"
ASM_DIR="target/asm"
BASELINE_DIR="$ASM_DIR/baseline"

if [[ "${1:-}" == "--baseline" ]]; then
    OUTPUT_DIR="$BASELINE_DIR"
else
    OUTPUT_DIR="$ASM_DIR"
fi

mkdir -p "$OUTPUT_DIR"

# Extract push/pop source for compilation
PUSH_SOURCE=$(cat <<'RUST'
use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::Cell;

#[repr(align(128))]
struct Padded<T>(T);

impl<T> core::ops::Deref for Padded<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.0 }
}

pub struct Ring {
    head: Padded<AtomicUsize>,
    tail: Padded<AtomicUsize>,
    tail_cached: Padded<Cell<usize>>,
    buf: *mut u64,
    mask: usize,
}

#[inline(never)]
#[no_mangle]
pub unsafe fn ring_push(ring: &Ring, value: u64) -> bool {
    let head = ring.head.load(Ordering::Relaxed);
    let next = (head + 1) & ring.mask;
    if next == ring.tail_cached.get() {
        let tail = ring.tail.load(Ordering::Acquire);
        ring.tail_cached.set(tail);
        if next == tail { return false; }
    }
    *ring.buf.add(head) = value;
    ring.head.store(next, Ordering::Release);
    true
}
RUST
)

# Compile for x86_64
echo "Compiling for x86_64..."
RESPONSE=$(curl -s -X POST "$GODBOLT_API/compiler/nightly/compile" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "$(jq -n \
        --arg source "$PUSH_SOURCE" \
        '{
            source: $source,
            options: {
                userArguments: "-C opt-level=3 -C target-cpu=x86-64-v3",
                filters: { intel: true, directives: true, commentOnly: true, labels: true }
            }
        }')")

echo "$RESPONSE" | jq -r '.asm[]?.text // empty' > "$OUTPUT_DIR/ring_push_x86_64.s"
echo "Saved: $OUTPUT_DIR/ring_push_x86_64.s"

# Compile for aarch64
echo "Compiling for aarch64..."
RESPONSE=$(curl -s -X POST "$GODBOLT_API/compiler/nightly/compile" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "$(jq -n \
        --arg source "$PUSH_SOURCE" \
        '{
            source: $source,
            options: {
                userArguments: "-C opt-level=3 --target aarch64-unknown-linux-gnu",
                filters: { directives: true, commentOnly: true, labels: true }
            }
        }')")

echo "$RESPONSE" | jq -r '.asm[]?.text // empty' > "$OUTPUT_DIR/ring_push_aarch64.s"
echo "Saved: $OUTPUT_DIR/ring_push_aarch64.s"

# Diff against baseline if exists
if [[ -d "$BASELINE_DIR" && "$OUTPUT_DIR" != "$BASELINE_DIR" ]]; then
    echo ""
    echo "=== Diff against baseline ==="
    for f in "$OUTPUT_DIR"/*.s; do
        base="$BASELINE_DIR/$(basename "$f")"
        if [[ -f "$base" ]]; then
            DIFF=$(diff "$base" "$f" || true)
            if [[ -n "$DIFF" ]]; then
                OLD_COUNT=$(wc -l < "$base" | tr -d ' ')
                NEW_COUNT=$(wc -l < "$f" | tr -d ' ')
                echo "CHANGED: $(basename "$f") ($OLD_COUNT -> $NEW_COUNT instructions)"
                echo "$DIFF"
            else
                echo "UNCHANGED: $(basename "$f")"
            fi
        fi
    done
fi

echo ""
echo "Done. Use --baseline to save current output as baseline."
```

- [ ] **Step 2: Make executable**

```bash
chmod +x scripts/check-asm.sh
```

- [ ] **Step 3: Test the script runs** (requires network)

Run: `./scripts/check-asm.sh --baseline`
Expected: Creates `target/asm/baseline/ring_push_x86_64.s` and `ring_push_aarch64.s`

- [ ] **Step 4: Commit**

```bash
git add scripts/check-asm.sh
git commit -m "feat(scripts): add Godbolt ASM inspection script"
```

---

## Task 11: Update `docs/PROGRESS.md`

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: Update Phase 1 Section 1.2 (Benchmark Harness) checkboxes**

Mark completed:
- [x] RDTSC + lfence cycle counter (x86_64)
- [x] kperf / `mach_absolute_time` counter (macOS ARM64)
- [x] `clock_gettime` counter (Linux ARM64)
- [x] Criterion integration with JSON export
- [x] Warmup phase and frequency stabilization
- [x] BenchReport with CPU name detection
- [x] External contender benchmarks (rtrb, crossbeam)
- [x] Benchmark workload shapes: single-item, burst, full-ring

Update crate status:
| `mantis-bench` | Active | std | ~10 | 6 benches | — |

- [ ] **Step 2: Commit**

```bash
git add docs/PROGRESS.md
git commit -m "docs: update PROGRESS.md with benchmark harness completion"
```

---

## Summary

| Task | What | Commit |
|---|---|---|
| 1 | Restructure counters module | `refactor(bench): restructure counters` |
| 2 | RDTSC counter (x86_64) | `feat(bench): add RdtscCounter` |
| 3 | KperfCounter (macOS ARM64) | `feat(bench): add KperfCounter` |
| 4 | PmuCounter (Linux ARM64) | `feat(bench): add PmuCounter` |
| 5 | MantisMeasurement criterion integration | `feat(bench): add MantisMeasurement` |
| 6 | BenchReport extension | `feat(bench): extend BenchReport` |
| 7 | Workload shapes | `feat(bench): add workload shapes` |
| 8 | Mantis criterion benchmarks | `feat(bench): add SPSC benchmarks` |
| 9 | Contender benchmarks (rtrb, crossbeam) | `feat(bench): add contender benchmarks` |
| 10 | Godbolt ASM script | `feat(scripts): add ASM inspection` |
| 11 | Progress doc update | `docs: update PROGRESS.md` |

**Total: 11 tasks, ~11 commits.**

**Note:** C++ contenders (Rigtorp, Drogalis) via FFI are deferred to a follow-up — they require vendoring headers, writing C wrappers, and `cc` build scripts. The Rust contenders (rtrb, crossbeam) provide sufficient competitive baseline for initial benchmarking.

After this plan: proceed to Plan 3 (Verification), Plan 4 (CI Improvements).
