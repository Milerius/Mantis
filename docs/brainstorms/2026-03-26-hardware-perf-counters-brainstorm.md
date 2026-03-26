# Hardware Performance Counters for mantis-bench

**Date:** 2026-03-26
**Status:** Approved

## What We're Building

Integrate hardware performance counters into mantis-bench so that benchmark reports populate the existing placeholder fields: `instructions_per_op`, `branch_misses_per_op`, `l1_misses_per_op`, `llc_misses_per_op`. Today only `cycles_per_op` and timing data are collected.

**Counters to support (6 total):**

| Counter | Linux source | macOS ARM64 source |
|---|---|---|
| CPU cycles | `perf_event_open` / RDTSC | kperf PMU / `mach_absolute_time` |
| Instructions retired | `perf_event_open` | kperf PMU |
| Branch misses | `perf_event_open` | kperf PMU |
| Branches retired | `perf_event_open` | kperf PMU |
| L1D cache misses | `perf_event_open` | None (kperf limitation) |
| LLC cache misses | `perf_event_open` | None (kperf limitation) |

## Why This Approach

**Inspiration:** [lemire/counters](https://github.com/lemire/counters) — a C++ library that wraps `perf_event_open` (Linux) and kperf (macOS ARM64) to measure cycles, instructions, branches, and branch misses. Our design extends this with L1/LLC cache counters via `perf-event` crate on Linux.

**Key insight from research:** lemire's macOS kperf backend uses private Apple frameworks (`kperf.framework`, `kperfdata.framework`) loaded via `dlopen` and requires root (`kpc_force_all_ctrs_set`). It programs the configurable PMU registers to read instructions and branch misses on Apple Silicon. We will port this to Rust with a runtime fallback when root is unavailable.

## Key Decisions

### 1. Extend `Measurement` struct (not new trait)

Add optional counter fields directly to `mantis_platform::metering::Measurement`:

```rust
pub struct Measurement {
    pub nanos: u64,
    pub cycles: u64,
    pub instructions: Option<u64>,
    pub branches: Option<u64>,
    pub branch_misses: Option<u64>,
    pub l1d_misses: Option<u64>,
    pub llc_misses: Option<u64>,
}
```

Rationale: Minimal API change. `CycleCounter::elapsed` returns richer data. Implementations that don't support extra counters return `None`. No new traits needed.

### 2. Use `perf-event` crate for Linux

The `perf-event` crate (maintained by Jim Blandy) provides idiomatic Rust wrappers around `perf_event_open` with `Group` support for reading multiple counters atomically. Supports `PERF_COUNT_HW_INSTRUCTIONS`, `PERF_COUNT_HW_BRANCH_MISSES`, `PERF_COUNT_HW_CACHE_L1D:READ:MISS`, and `PERF_COUNT_HW_CACHE_LL:READ:MISS`.

Added as a dependency of `mantis-platform` behind the `perf-counters` feature flag (Linux only).

### 3. Port kperf PMU for macOS ARM64

Port lemire's `apple_arm_events.h` to Rust:
- `dlopen` kperf + kperfdata frameworks
- Query CPU event database via `kpep_db_create` / `kpep_db_event`
- Program PMU via `kpc_set_config` / `kpc_get_thread_counters`
- Event aliases with fallback chains (e.g., `BRANCH_MISPRED_NONSPEC` → `BRANCH_MISPREDICT`)

**Runtime fallback:** If not running as root (or `kpc_force_all_ctrs_set` fails), gracefully fall back to cycles-only (`mach_absolute_time`). Extra counter fields return `None`.

**Feature flag:** Behind `perf-counters` feature. Without it, macOS stays on current `mach_absolute_time`-only path.

### 4. Feature flag: `perf-counters`

```toml
# mantis-platform/Cargo.toml
[features]
perf-counters = ["dep:perf-event"]  # perf-event only on Linux

# mantis-bench/Cargo.toml
[features]
perf-counters = ["mantis-platform/perf-counters"]
```

- Default builds: no extra deps, `None` for extended counters
- Bench CI: `cargo bench --features perf-counters`
- Local with sudo: `sudo cargo bench --features perf-counters` (macOS full counters)
- Local without sudo: `cargo bench --features perf-counters` (macOS falls back to cycles-only)

### 5. Platform support matrix

| Platform | Cycles | Instructions | Branches | Branch Misses | L1D Misses | LLC Misses |
|---|---|---|---|---|---|---|
| Linux x86_64 (perf-counters) | RDTSC | perf_event | perf_event | perf_event | perf_event | perf_event |
| Linux x86_64 (no flag) | RDTSC | None | None | None | None | None |
| macOS ARM64 (perf-counters + root) | kperf PMU | kperf PMU | kperf PMU | kperf PMU | None | None |
| macOS ARM64 (perf-counters, no root) | mach_absolute_time | None | None | None | None | None |
| macOS ARM64 (no flag) | mach_absolute_time | None | None | None | None | None |
| Fallback (std) | Instant | None | None | None | None | None |

### 6. Integration with criterion

The `SampleCollector` thread-local in `measurement.rs` currently stores `(cycles, nanos)` per sample. Extend to store the full `Measurement` struct. `MantisMeasurement` passes the extra counters through to `BenchDesc` which populates `WorkloadResult`.

### 7. CI bench workflow

Update bench.yml to enable `perf-counters`:
```yaml
- name: Run SPSC benchmarks
  run: cargo +nightly bench --bench spsc --all-features
  env:
    RUSTFLAGS: "-C target-cpu=native"
```

`--all-features` already includes `perf-counters`. Linux CI runners support `perf_event_open` (may need `perf_event_paranoid` check). macOS CI runners won't have root, so they gracefully degrade to cycles-only.

## Resolved Questions

- **Scope:** Full 6 counters (not just lemire's 4)
- **Architecture:** Extend Measurement struct, not new trait
- **Linux backend:** Use `perf-event` crate
- **macOS backend:** Full kperf PMU port with runtime root fallback
- **Feature flag:** `perf-counters` in mantis-platform and mantis-bench

## Resolved Questions (continued)

- **CI fallback:** Graceful `None` fallback if `perf_event_open` fails (paranoid >= 2). Never crash, just degrade to cycles + time.
- **Counter overhead:** Rely on criterion's existing iteration batching. Read counters once per batch (start/end around `b.iter` block), divide by iteration count. No inner-loop repeat needed.
