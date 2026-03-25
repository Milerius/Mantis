# mantis-bench

Benchmark harness and performance counter utilities for the Mantis SDK.

`std`-only tooling crate.

## Architecture

```
┌────────────────────────────────────────────┐
│          Benchmark Binaries                │
│  spsc_mantis.rs    spsc_contenders.rs      │
└──────────────┬─────────────────────────────┘
               │ uses
┌──────────────▼─────────────────────────────┐
│           bench_runner                      │
│  MantisC · run_bench · export_report       │
└──────┬────────────┬────────────────────────┘
       │            │
┌──────▼──────┐  ┌──▼────────────────────────┐
│ measurement │  │        report              │
│ Mantis      │  │  BenchReport               │
│ Measurement │  │  WorkloadResult             │
│ + Criterion │  │  → JSON export             │
└──────┬──────┘  └────────────────────────────┘
       │
┌──────▼──────────────────────────────────────┐
│              counters                        │
│  CycleCounter trait                          │
│  ┌──────────┬──────────┬──────────┬────────┐│
│  │ RdtscCtr │ KperfCtr │ PmuCtr   │Instant ││
│  │ x86_64   │ macOS    │ Linux    │fallback││
│  │ +asm     │ ARM64    │ ARM64    │        ││
│  └──────────┴──────────┴──────────┴────────┘│
└─────────────────────────────────────────────┘
```

### How It Works

1. `MantisMeasurement<C>` implements criterion's `Measurement` trait
2. On each sample, it captures CPU cycles via the platform counter alongside wall time
3. After all benchmarks, `export_report` reads criterion's `estimates.json` for accurate per-iteration wall-time stats
4. Combines wall-time + cycle data into `BenchReport` JSON

### Platform Counter Selection

| Platform | Counter | Method |
|---|---|---|
| x86_64 + `asm` feature | `RdtscCounter` | `rdtsc` + `lfence` serialization |
| aarch64 macOS | `KperfCounter` | `mach_absolute_time()` |
| aarch64 Linux | `PmuCounter` | `clock_gettime(CLOCK_MONOTONIC)` |
| Everything else | `InstantCounter` | `std::time::Instant` |

## Running Benchmarks

```bash
# Mantis SPSC benchmarks
cargo bench -p mantis-bench --bench spsc_mantis

# With external contenders (rtrb, crossbeam)
cargo bench -p mantis-bench --bench spsc_contenders --features bench-contenders

# Quick mode (fewer iterations, faster feedback)
cargo bench -p mantis-bench -- --quick
```

### Output

- Console: summary table with ns/op, ops/s, p50, p99, cycles/op
- JSON: `target/bench-report-mantis.json` and `target/bench-report-contenders.json`
- Criterion HTML: `target/criterion/` (open `report/index.html`)

## Report Schema

Each `WorkloadResult` in the JSON report contains:

| Field | Type | Source |
|---|---|---|
| `ops_per_sec` | f64 | criterion estimates |
| `ns_per_op` | f64 | criterion estimates |
| `p50_ns` | f64 | criterion median |
| `p99_ns` | f64 | Gaussian approximation from mean + 2.326 * std_dev |
| `p999_ns` | f64 | Gaussian approximation from mean + 3.09 * std_dev |
| `cycles_per_op` | Option\<f64\> | platform cycle counter |
| `instructions_per_op` | Option\<f64\> | reserved (perf_event) |
| `branch_misses_per_op` | Option\<f64\> | reserved (perf_event) |
| `l1_misses_per_op` | Option\<f64\> | reserved (perf_event) |
| `llc_misses_per_op` | Option\<f64\> | reserved (perf_event) |
| `full_rate` | Option\<f64\> | instrumented preset |
| `empty_rate` | Option\<f64\> | instrumented preset |
| `mean_occupancy` | Option\<f64\> | instrumented preset |

## ASM Inspection

Inspect the generated assembly for hot-path functions:

```bash
# Requires: cargo install cargo-show-asm
./scripts/check-asm.sh

# Save baseline for future diffs
./scripts/check-asm.sh --baseline

# Inspect a specific symbol
./scripts/check-asm.sh --symbol "asm_shim::spsc_push_u64"
```

## Features

| Feature | Default | Effect |
|---|---|---|
| `asm` | off | Enables RDTSC cycle counter on x86_64 |
| `bench-contenders` | off | Enables rtrb + crossbeam contender benchmarks |

## Modules

| Module | Purpose |
|---|---|
| `counters` | Platform cycle counter trait + implementations |
| `measurement` | Criterion `Measurement` integration + sample collector |
| `report` | `BenchReport` / `WorkloadResult` JSON schema |
| `bench_runner` | Shared benchmark harness (`run_bench`, `export_report`) |
| `workloads` | Standardized workload shapes (single-item, burst, full-drain) |
