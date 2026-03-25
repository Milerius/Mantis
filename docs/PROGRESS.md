# Mantis SDK — Project Progress

> This document tracks the global advancement of the Mantis SDK.
> Agents must update this file when completing meaningful work.
> See `philosophy/fin_sdk_oss_blueprint.md` for full roadmap details.

---

## Phase 0 — Project Bootstrap

**Status: Complete** | Completed: 2026-03-25

- [x] Rust workspace with crate skeletons (`mantis-core`, `mantis-types`, `mantis-queue`, `mantis-bench`, `mantis-layout`, `mantis-verify`)
- [x] CI pipeline: fmt, clippy, test, no_std test, doc, deny, miri, careful, coverage
- [x] Nightly CI: mutants, extended miri, full coverage, ASM toggle
- [x] Verification CI: kani proofs, bolero property tests
- [x] Benchmark regression CI with artifact upload
- [x] Dependabot for cargo + actions
- [x] Coding guidelines (`CLAUDE.md`)
- [x] Unsafe policy (`docs/UNSAFE.md`)
- [x] Justfile task runner
- [x] Toolchain configs: `rust-toolchain.toml`, `clippy.toml`, `deny.toml`
- [x] Strategy pattern traits: `IndexStrategy`, `PushPolicy`, `Instrumentation`
- [x] Core newtypes: `SeqNum`, `SlotIndex`, `QueueError`
- [x] Benchmark counter trait + `InstantCounter` fallback
- [x] Layout inspector CLI
- [x] Constantine reference patterns documented

**Not in scope (deferred):**
- API stability policy draft
- Fuzz harness skeleton (placeholder only)
- Topology visualization

---

## Phase 1 — Minimal Useful Core

**Status: Not Started**

### 1.1 SPSC Ring Buffer (`mantis-queue`)
- [ ] Core ring buffer engine with strategy pattern
- [ ] `raw` submodule with unsafe slot operations
- [ ] Power-of-2 masked index implementation
- [ ] Cache-padded variant to prevent false sharing
- [ ] Portable baseline implementation
- [ ] Platform-specific atomics (x86_64 / ARM64)
- [ ] Preset type aliases (`SpscRingPortable`, `SpscRingPadded`)
- [ ] Unit tests (push/pop, full/empty, wraparound)
- [ ] Miri validation
- [ ] Kani bounded model checking proofs
- [ ] Bolero property-based tests
- [ ] Differential testing across strategy variants

### 1.2 Benchmark Harness (`mantis-bench`)
- [ ] RDTSC + lfence cycle counter (x86_64)
- [ ] kperf / `mach_absolute_time` counter (macOS ARM64)
- [ ] `clock_gettime` counter (Linux ARM64)
- [ ] Criterion integration with JSON export
- [ ] Warmup phase and frequency stabilization
- [ ] BenchReport with CPU name detection
- [ ] External contender benchmarks via FFI (`bench-contenders` feature)
  - [ ] rtrb
  - [ ] ringbuf
  - [ ] crossbeam
- [ ] Benchmark workload shapes: single-item, burst, full-ring

### 1.3 Canonical Event Model
- [ ] `BookDelta`, `Trade`, `Quote` types
- [ ] `OrderIntent`, `Fill` types
- [ ] `OracleUpdate`, funding types
- [ ] Event enum with discriminant
- [ ] no_std compatible serialization

### 1.4 Snapshot Publication
- [ ] Single-writer publication primitive
- [ ] Lock-free reader access

### 1.5 Order Book Engine
- [ ] Single-writer order book engine
- [ ] Level management (add/remove/modify)
- [ ] Top-of-book query
- [ ] Benchmark vs naive implementations

### 1.6 Capture / Replay
- [ ] Capture file format v0
- [ ] Writer / reader implementations
- [ ] Replay runner feeding events into engines
- [ ] Deterministic replay validation

### 1.7 Tooling
- [ ] Layout report for all hot-path structs
- [ ] First replay diff format
- [ ] Fuzz targets for SPSC ring + event parsing

**Exit criteria:**
- Stable end-to-end: capture -> replay -> state update -> output
- p50/p99 benchmark output exists
- First invariant tests pass
- First docs/examples usable by outside readers

---

## Phase 2 — First Compelling OSS Release Candidate

**Status: Not Started**

- [ ] Constant-product AMM engine MVP
- [ ] Perp state / funding / risk primitives MVP
- [ ] Visualizer: stage graph, queue depths, latency timeline
- [ ] Parser/engine fuzz targets
- [ ] Synthetic exchange replay example
- [ ] Synthetic AMM/perp replay example
- [ ] Queue occupancy instrumentation
- [ ] Stage timing capture
- [ ] Divergence report between two replay runs

---

## Phase 3 — Fast OSS v0.1 Release

**Status: Not Started**

- [ ] Documentation pass
- [ ] Public architecture diagrams
- [ ] Contribution guide
- [ ] Versioning policy + changelog
- [ ] Benchmark baseline published
- [ ] 2-3 example applications (CEX book replay, DEX route, perp monitor)
- [ ] README with demo GIFs/screenshots

---

## Crate Status Summary

| Crate | Status | no_std | Tests | Benchmarks | Verification |
|---|---|---|---|---|---|
| `mantis-core` | Scaffold | yes | 0 | — | — |
| `mantis-types` | Scaffold | yes | 4 | — | — |
| `mantis-queue` | Scaffold | yes | 0 | — | — |
| `mantis-bench` | Scaffold | std | 2 | — | — |
| `mantis-layout` | Scaffold | std | 2 | — | — |
| `mantis-verify` | Scaffold | std | 1 | — | — |
