---
title: "feat: Hardware Performance Counters for mantis-bench"
type: feat
status: active
date: 2026-03-26
---

# Hardware Performance Counters Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate the existing placeholder fields (`instructions_per_op`, `branch_misses_per_op`, `l1_misses_per_op`, `llc_misses_per_op`) in benchmark reports using hardware performance counters.

**Architecture:** Extend `Measurement` struct with optional counter fields. Add `perf-event` crate on Linux for grouped hardware counters. Port lemire's kperf PMU approach to Rust for macOS ARM64. Feature-gated behind `perf-counters` with graceful `None` fallback.

**Tech Stack:** `perf-event` crate (v0.4.9, Linux), kperf/kperfdata private frameworks (macOS), `cfg_if` for platform dispatch.

**Brainstorm:** `docs/brainstorms/2026-03-26-hardware-perf-counters-brainstorm.md`

---

## File Structure

```
crates/platform/Cargo.toml                    — add perf-event dep + perf-counters feature
crates/platform/src/metering/mod.rs           — extend Measurement, add HwCounters trait
crates/platform/src/metering/hw_counters.rs   — NEW: HwCounters trait + NoopCounters
crates/platform/src/metering/perf_group.rs    — NEW: Linux perf-event grouped counter impl
crates/platform/src/metering/kperf_pmu.rs     — NEW: macOS kperf PMU counter impl
crates/bench/Cargo.toml                       — propagate perf-counters feature
crates/bench/src/measurement.rs               — extend SampleCollector + MantisMeasurement
crates/bench/src/bench_runner.rs              — wire HwCounters through BenchDesc → report
crates/bench/benches/spsc.rs                  — add perf-counters feature to report
.github/workflows/bench.yml                   — already uses --all-features, no change needed
```

---

### Task 1: Extend `Measurement` struct with optional counter fields

**Files:**
- Modify: `crates/platform/src/metering/mod.rs:20-26`

- [ ] **Step 1: Add optional fields to Measurement**

```rust
/// A measurement from a performance counter.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Wall-clock duration in nanoseconds.
    pub nanos: u64,
    /// CPU cycles (if available on this platform, else 0).
    pub cycles: u64,
    /// Instructions retired (if available).
    pub instructions: Option<u64>,
    /// Branch misses (if available).
    pub branch_misses: Option<u64>,
    /// L1D cache read misses (if available).
    pub l1d_misses: Option<u64>,
    /// Last-level cache read misses (if available).
    pub llc_misses: Option<u64>,
}
```

- [ ] **Step 2: Update all Measurement construction sites**

Every place that constructs a `Measurement { nanos, cycles }` must add the new fields as `None`. There are 4 sites:

1. `crates/platform/src/isa_arm64/counters.rs:92` — `KperfCounter::elapsed`
2. `crates/platform/src/isa_arm64/counters.rs:153` — `PmuCounter::elapsed`
3. `crates/platform/src/isa_x86/rdtsc.rs:67` — `RdtscCounter::elapsed`
4. `crates/platform/src/metering/instant.rs:36` — `InstantCounter::elapsed`

Each becomes:
```rust
Measurement {
    nanos: ...,
    cycles: ...,
    instructions: None,
    branch_misses: None,
    l1d_misses: None,
    llc_misses: None,
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build --features alloc,std`
Expected: compiles cleanly

- [ ] **Step 4: Run tests**

Run: `cargo test --features alloc,std`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/platform/src/metering/mod.rs crates/platform/src/isa_arm64/counters.rs crates/platform/src/isa_x86/rdtsc.rs crates/platform/src/metering/instant.rs
git commit -m "feat(platform): extend Measurement with optional hw counter fields"
```

---

### Task 2: Add `HwCounters` trait and noop implementation

**Files:**
- Create: `crates/platform/src/metering/hw_counters.rs`
- Modify: `crates/platform/src/metering/mod.rs`

- [ ] **Step 1: Write tests for NoopCounters**

Add to `hw_counters.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_counters_returns_none() {
        let c = NoopCounters;
        assert!(c.start().is_none());
        assert!(c.read(&None).is_none());
    }
}
```

- [ ] **Step 2: Define the trait and noop implementation**

```rust
//! Hardware performance counter abstraction.

/// A snapshot of hardware counter state (opaque, platform-specific).
///
/// Implementations return `None` from all methods when counters are
/// unavailable (no root, paranoid >= 2, unsupported platform).
pub trait HwCounters: Send + Sync {
    /// Opaque counter state captured at measurement start.
    type Snapshot;

    /// Capture counter state. Returns `None` if counters unavailable.
    fn start(&self) -> Option<Self::Snapshot>;

    /// Read delta since snapshot. Returns per-counter deltas.
    fn read(&self, snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas>;
}

/// Deltas from hardware counters for a single measurement interval.
#[derive(Debug, Clone, Copy, Default)]
pub struct HwCounterDeltas {
    /// Instructions retired.
    pub instructions: u64,
    /// Branch misses.
    pub branch_misses: u64,
    /// L1D cache read misses.
    pub l1d_misses: u64,
    /// LLC read misses.
    pub llc_misses: u64,
}

/// No-op counters for platforms without hardware counter support.
pub struct NoopCounters;

impl HwCounters for NoopCounters {
    type Snapshot = ();

    fn start(&self) -> Option<Self::Snapshot> {
        None
    }

    fn read(&self, _snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas> {
        None
    }
}
```

- [ ] **Step 3: Wire into metering/mod.rs**

Add `pub mod hw_counters;` and re-export `HwCounters`, `HwCounterDeltas`, `NoopCounters`.

Add a `DefaultHwCounters` type alias (initially `NoopCounters` for all platforms — updated in later tasks):
```rust
cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "perf-counters"))] {
        pub type DefaultHwCounters = hw_counters::PerfGroupCounters;
    } else if #[cfg(all(target_os = "macos", target_arch = "aarch64", feature = "perf-counters"))] {
        pub type DefaultHwCounters = hw_counters::KperfPmuCounters;
    } else {
        pub type DefaultHwCounters = hw_counters::NoopCounters;
    }
}
```

Note: `PerfGroupCounters` and `KperfPmuCounters` don't exist yet — guard with `cfg` so this compiles. Initially all paths resolve to `NoopCounters` until Tasks 4-5 add the real implementations.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mantis-platform --features std`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/platform/src/metering/hw_counters.rs crates/platform/src/metering/mod.rs
git commit -m "feat(platform): add HwCounters trait and NoopCounters"
```

---

### Task 3: Extend `SampleCollector` and `MantisMeasurement` in mantis-bench

**Files:**
- Modify: `crates/bench/src/measurement.rs`
- Modify: `crates/bench/src/bench_runner.rs`
- Modify: `crates/bench/src/report.rs` (no struct changes, just wiring)

- [ ] **Step 1: Extend SampleCollector with optional hw counter vectors**

```rust
#[derive(Debug, Default)]
pub struct SampleCollector {
    pub cycles: Vec<u64>,
    pub nanos: Vec<u64>,
    pub instructions: Vec<u64>,
    pub branch_misses: Vec<u64>,
    pub l1d_misses: Vec<u64>,
    pub llc_misses: Vec<u64>,
    /// Whether hw counters were collected for these samples.
    pub has_hw_counters: bool,
}
```

Update `reset()` and `len()`/`is_empty()` accordingly. Add mean helpers:

```rust
pub fn mean_instructions_per_sample(&self) -> Option<f64> { ... }
pub fn mean_branch_misses_per_sample(&self) -> Option<f64> { ... }
pub fn mean_l1d_misses_per_sample(&self) -> Option<f64> { ... }
pub fn mean_llc_misses_per_sample(&self) -> Option<f64> { ... }
```

Each returns `None` if `!self.has_hw_counters || self.instructions.is_empty()`.

- [ ] **Step 2: Update MantisMeasurement to accept HwCounters**

Add a type parameter or store an `Option<DefaultHwCounters>`:

```rust
pub struct MantisMeasurement<C: CycleCounter> {
    counter: C,
    wall: WallTime,
    hw: Option<DefaultHwCounters>,
}
```

In `start()`, also call `hw.as_ref().and_then(|h| h.start())` and store the snapshot in `Intermediate`.

In `end()`, read hw counter deltas and push to `SampleCollector`.

Change `Intermediate` to include the hw snapshot:
```rust
type Intermediate = (u64, std::time::Instant, Option<...>);
```

- [ ] **Step 3: Update `DefaultMeasurement::platform_default()`**

Conditionally create `DefaultHwCounters`:
```rust
pub fn platform_default() -> Self {
    let hw = DefaultHwCounters::try_new().ok();
    Self {
        counter: DefaultCounter::default(),
        wall: WallTime,
        hw,
    }
}
```

Where `try_new()` is the fallible constructor (returns `Err` if counters unavailable).

- [ ] **Step 4: Extend BenchDesc with hw counter means**

```rust
pub struct BenchDesc {
    pub id: &'static str,
    pub element_type: &'static str,
    pub capacity: usize,
    pub mean_cycles_per_sample: Option<f64>,
    pub mean_instructions_per_sample: Option<f64>,
    pub mean_branch_misses_per_sample: Option<f64>,
    pub mean_l1d_misses_per_sample: Option<f64>,
    pub mean_llc_misses_per_sample: Option<f64>,
}
```

- [ ] **Step 5: Wire into export_report**

Replace the `None` placeholders in `bench_runner.rs:99-102`:
```rust
instructions_per_op: desc.mean_instructions_per_sample,
branch_misses_per_op: desc.mean_branch_misses_per_sample,
l1_misses_per_op: desc.mean_l1d_misses_per_sample,
llc_misses_per_op: desc.mean_llc_misses_per_sample,
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p mantis-bench`
Expected: PASS (all None since no perf-counters feature yet)

- [ ] **Step 7: Commit**

```bash
git add crates/bench/src/measurement.rs crates/bench/src/bench_runner.rs
git commit -m "feat(bench): extend SampleCollector and BenchDesc for hw counters"
```

---

### Task 4: Linux `perf-event` grouped counter implementation

**Files:**
- Modify: `crates/platform/Cargo.toml` — add `perf-event` dep
- Modify: `Cargo.toml` (workspace) — add `perf-event` to workspace deps
- Create: `crates/platform/src/metering/perf_group.rs`
- Modify: `crates/platform/src/metering/mod.rs` — conditionally include module

- [ ] **Step 1: Add perf-event dependency**

In workspace `Cargo.toml`:
```toml
perf-event = "0.4"
```

In `crates/platform/Cargo.toml`:
```toml
[features]
perf-counters = ["dep:perf-event"]

[target.'cfg(target_os = "linux")'.dependencies]
perf-event = { workspace = true, optional = true }
```

- [ ] **Step 2: Implement PerfGroupCounters**

```rust
//! Linux perf_event grouped hardware counters.

use perf_event::{Builder, Counter, Group};
use perf_event::events::Hardware;
use crate::metering::hw_counters::{HwCounters, HwCounterDeltas};

/// Grouped hardware counters via Linux `perf_event_open`.
pub struct PerfGroupCounters {
    group: Group,
    // Individual counter handles for reading
    instructions: Counter,
    branch_misses: Counter,
    l1d_misses: Counter,
    llc_misses: Counter,
}

impl PerfGroupCounters {
    /// Try to create a perf counter group.
    ///
    /// Returns `Err` if `perf_event_open` fails (paranoid >= 2, no permissions).
    pub fn try_new() -> Result<Self, std::io::Error> {
        let mut group = Group::new()?;
        let instructions = Builder::new(Hardware::INSTRUCTIONS)
            .group(&mut group)
            .build()?;
        let branch_misses = Builder::new(Hardware::BRANCH_MISSES)
            .group(&mut group)
            .build()?;
        let l1d_misses = Builder::new(Hardware::CACHE_MISSES) // L1D approximation
            .group(&mut group)
            .build()?;
        // LLC misses via cache event
        let llc_misses = Builder::new(Hardware::CACHE_MISSES)
            .group(&mut group)
            .build()?;
        Ok(Self {
            group,
            instructions,
            branch_misses,
            l1d_misses,
            llc_misses,
        })
    }
}
```

Note: The actual L1D and LLC cache events use `perf_event::events::Cache` with specific config. The exact API:

```rust
use perf_event::events::{Cache, CacheOp, CacheResult, WhichCache};

// L1D read misses
let l1d_misses = Builder::new(Cache {
    which: WhichCache::L1D,
    operation: CacheOp::Read,
    result: CacheResult::Miss,
}).group(&mut group).build()?;

// LLC read misses
let llc_misses = Builder::new(Cache {
    which: WhichCache::LL,
    operation: CacheOp::Read,
    result: CacheResult::Miss,
}).group(&mut group).build()?;
```

Implement `HwCounters`:
```rust
impl HwCounters for PerfGroupCounters {
    type Snapshot = perf_event::CounterGroup; // or Vec<u64>

    fn start(&self) -> Option<Self::Snapshot> {
        self.group.enable().ok()?;
        // Read initial values
        self.group.read().ok()
    }

    fn read(&self, snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas> {
        self.group.disable().ok()?;
        let counts = self.group.read().ok()?;
        let base = snapshot.as_ref()?;
        Some(HwCounterDeltas {
            instructions: counts[&self.instructions] - base[&self.instructions],
            branch_misses: counts[&self.branch_misses] - base[&self.branch_misses],
            l1d_misses: counts[&self.l1d_misses] - base[&self.l1d_misses],
            llc_misses: counts[&self.llc_misses] - base[&self.llc_misses],
        })
    }
}
```

- [ ] **Step 3: Wire into metering/mod.rs**

Add conditional module include:
```rust
#[cfg(all(target_os = "linux", feature = "perf-counters"))]
pub mod perf_group;
```

Update `DefaultHwCounters` type alias to use `PerfGroupCounters` on Linux.

- [ ] **Step 4: Verify Linux compilation**

Run: `cargo build --features alloc,std,perf-counters` (on Linux) or `cargo check --features alloc,std` (on macOS — perf-event not included, `DefaultHwCounters` stays `NoopCounters`)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/platform/Cargo.toml crates/platform/src/metering/perf_group.rs crates/platform/src/metering/mod.rs
git commit -m "feat(platform): add Linux perf-event grouped hardware counters"
```

---

### Task 5: macOS kperf PMU counter implementation

**Files:**
- Create: `crates/platform/src/metering/kperf_pmu.rs`
- Modify: `crates/platform/src/metering/mod.rs`

This is the most complex task. Port lemire's `apple_arm_events.h` approach to Rust.

- [ ] **Step 1: Implement kperf FFI bindings**

Key functions to bind via `dlopen`:
```rust
// kperf.framework
type KpcForceAllCtrsSetFn = unsafe extern "C" fn(i32) -> i32;
type KpcSetConfigFn = unsafe extern "C" fn(u32, *mut u64) -> i32;
type KpcGetThreadCountersFn = unsafe extern "C" fn(u32, u32, *mut u64) -> i32;
type KpcSetCountingFn = unsafe extern "C" fn(u32) -> i32;
type KpcSetThreadCountingFn = unsafe extern "C" fn(u32) -> i32;
type KpcGetCounterCountFn = unsafe extern "C" fn(u32) -> u32;
type KpcGetConfigCountFn = unsafe extern "C" fn(u32) -> u32;

// kperfdata.framework
type KpepDbCreateFn = unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32;
type KpepDbEventFn = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void) -> i32;
type KpepEventIdFn = unsafe extern "C" fn(*mut c_void, *mut u64) -> i32;
```

Load via `dlopen("/System/Library/PrivateFrameworks/kperf.framework/kperf", RTLD_LAZY)`.

- [ ] **Step 2: Implement event lookup with alias fallback**

Event alias chains (Apple Silicon generational differences):
```
Instructions: FIXED_INSTRUCTIONS → INST_ALL
Branch misses: BRANCH_MISPRED_NONSPEC → BRANCH_MISPREDICT
```

- [ ] **Step 3: Implement KperfPmuCounters**

```rust
pub struct KperfPmuCounters {
    counter_offset: usize,  // offset to configurable counters
    // Indices into the counter array for our events
    instructions_idx: usize,
    branch_misses_idx: usize,
}

impl KperfPmuCounters {
    pub fn try_new() -> Result<Self, KperfError> {
        // 1. dlopen kperf + kperfdata frameworks
        // 2. kpc_force_all_ctrs_set(1) — requires root
        // 3. kpep_db_create(null, &db) — load CPU event database
        // 4. Look up events by name with fallback chains
        // 5. kpc_set_config + kpc_set_counting + kpc_set_thread_counting
        // Returns Err if any step fails (not root, framework not found, etc.)
    }
}

impl HwCounters for KperfPmuCounters {
    type Snapshot = Vec<u64>;

    fn start(&self) -> Option<Self::Snapshot> {
        // kpc_get_thread_counters(KPC_MASK, counter_count, &counters)
    }

    fn read(&self, snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas> {
        // Read current, subtract snapshot
        // Note: L1D and LLC not available via kperf — return 0
    }
}
```

- [ ] **Step 4: Wire into mod.rs**

```rust
#[cfg(all(target_os = "macos", target_arch = "aarch64", feature = "perf-counters"))]
pub mod kperf_pmu;
```

Update `DefaultHwCounters` cfg for macOS.

- [ ] **Step 5: Test on macOS (manual)**

Run: `sudo cargo test -p mantis-platform --features std,perf-counters`
Expected: counters read non-zero values for instructions and branch misses.
Without sudo: `try_new()` returns Err, falls back to NoopCounters.

- [ ] **Step 6: Commit**

```bash
git add crates/platform/src/metering/kperf_pmu.rs crates/platform/src/metering/mod.rs
git commit -m "feat(platform): add macOS kperf PMU hardware counters"
```

---

### Task 6: Feature flag propagation and bench integration

**Files:**
- Modify: `crates/bench/Cargo.toml`
- Modify: `crates/bench/benches/spsc.rs`
- Modify: `Cargo.toml` (workspace, if needed)

- [ ] **Step 1: Add perf-counters feature to mantis-bench**

```toml
# crates/bench/Cargo.toml
[features]
perf-counters = ["mantis-platform/perf-counters"]
```

- [ ] **Step 2: Report perf-counters in spsc.rs feature list**

In `bench_spsc()` function at `crates/bench/benches/spsc.rs:376-381`:
```rust
if cfg!(feature = "perf-counters") {
    features.push("perf-counters".to_owned());
}
```

- [ ] **Step 3: Run benchmarks locally**

Run: `cargo bench --bench spsc --features perf-counters`
Expected: JSON report includes hw counter fields (non-None on Linux, None on macOS without root)

- [ ] **Step 4: Verify CI compatibility**

The `.github/workflows/bench.yml` already uses `--all-features`, which will include `perf-counters`. On CI:
- Linux: `perf_event_open` may fail if `perf_event_paranoid >= 2` → graceful `None` fallback
- macOS: no root → graceful `None` fallback

No changes needed to bench.yml.

- [ ] **Step 5: Commit**

```bash
git add crates/bench/Cargo.toml crates/bench/benches/spsc.rs
git commit -m "feat(bench): wire perf-counters feature flag through to benchmarks"
```

---

### Task 7: Enhanced benchmark report output

**Files:**
- Modify: `crates/bench/src/bench_runner.rs`

- [ ] **Step 1: Add hw counter info to stderr report**

Update the `eprintln!` formatting in `export_report` to show hw counters when available:

```rust
if let Some(instr) = r.instructions_per_op {
    eprint!("  instr={instr:.0}");
}
if let Some(bm) = r.branch_misses_per_op {
    eprint!("  br_miss={bm:.2}");
}
if let Some(l1) = r.l1_misses_per_op {
    eprint!("  l1_miss={l1:.2}");
}
if let Some(llc) = r.llc_misses_per_op {
    eprint!("  llc_miss={llc:.2}");
}
```

- [ ] **Step 2: Run full bench and verify output**

Run: `cargo bench --bench spsc`
Expected: report prints, hw counter fields show `None` (or values if `perf-counters` enabled)

- [ ] **Step 3: Commit**

```bash
git add crates/bench/src/bench_runner.rs
git commit -m "feat(bench): display hardware counter metrics in benchmark report"
```

---

## Platform Support Matrix

| Platform | Cycles | Instructions | Branch Misses | L1D Misses | LLC Misses |
|---|---|---|---|---|---|
| Linux x86_64 (perf-counters) | RDTSC | perf_event | perf_event | perf_event | perf_event |
| Linux x86_64 (no flag) | RDTSC | None | None | None | None |
| macOS ARM64 (perf-counters + root) | kperf | kperf PMU | kperf PMU | None | None |
| macOS ARM64 (perf-counters, no root) | mach_absolute_time | None | None | None | None |
| macOS ARM64 (no flag) | mach_absolute_time | None | None | None | None |
| Fallback (std) | Instant | None | None | None | None |

## Key Design Decisions

1. **Extend `Measurement` struct** — not a new trait. Minimal API surface change.
2. **Separate `HwCounters` trait** — decoupled from `CycleCounter` because hw counters have different lifecycle (group enable/disable vs single timestamp).
3. **`perf-event` crate for Linux** — maintained, idiomatic Rust, supports grouped counters atomically.
4. **kperf PMU for macOS** — port of lemire/counters approach; requires root; runtime fallback.
5. **Feature flag `perf-counters`** — no extra deps or code in default builds.
6. **Graceful fallback everywhere** — never crash, always degrade to `None`.
