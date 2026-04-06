# Mantis SDK — Project Progress

> This document tracks the global advancement of the Mantis SDK.
> Agents must update this file when completing meaningful work.
> See `philosophy/fin_sdk_oss_blueprint.md` for full roadmap details.

---

## Phase 0 — Project Bootstrap

**Status: Complete** | Completed: 2026-03-25

- [x] Rust workspace with crate skeletons (`mantis-core`, `mantis-types`, `mantis-queue`, `mantis-bench`, `mantis-layout`, `mantis-verify`)
- [x] CI pipeline: fmt, clippy, nextest, no_std test, doc, deny, miri, careful, coverage, codecov
- [x] Nightly CI: mutants, extended miri, full coverage, ASM toggle, ASM inspection, kani proofs, fuzz
- [x] Verification CI: kani proofs (4), bolero property tests (4), differential tests (3)
- [x] Benchmark regression CI with 5% threshold, artifact upload, PR annotations
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

**Status: In Progress** | Started: 2026-03-25

### 1.1 SPSC Ring Buffer (`mantis-queue`)
**Status: Complete**

- [x] Core ring buffer engine with strategy pattern
- [x] `raw` submodule with unsafe slot operations
- [x] Power-of-2 masked index implementation
- [x] Cache-padded variant to prevent false sharing
- [x] Portable baseline implementation
- [x] Platform-specific atomics (x86_64 / ARM64)
- [x] Preset type aliases (`SpscRing`, `SpscRingHeap`, `SpscRingInstrumented`)
- [x] Unit tests (23 unit + 7 integration + 1 stress)
- [x] Miri validation (31/31 tests pass, zero UB)
- [x] Kani bounded model checking proofs (4 proofs)
- [x] Bolero property-based tests (4 properties)
- [x] Differential testing across strategy variants (3 comparisons)

### 1.2 Benchmark Harness (`mantis-bench`)
**Status: Complete**

- [x] RDTSC + lfence cycle counter (x86_64)
- [x] kperf / `mach_absolute_time` counter (macOS ARM64)
- [x] `clock_gettime` counter (Linux ARM64)
- [x] Criterion integration with JSON export
- [x] MantisMeasurement<C> criterion Measurement trait
- [x] BenchReport with CPU name, compiler, full metrics schema
- [x] External contender benchmarks (`bench-contenders` feature)
  - [x] rtrb
  - [x] crossbeam
- [x] Benchmark workload shapes: single-item, burst, full-drain
- [x] Godbolt ASM inspection script

### 1.8 Platform Abstractions (`mantis-platform`)
**Status: Complete**

- [x] Constant-time types (Ct<T>, CTBool<T>, Carry, Borrow)
- [x] Constant-time arithmetic (ct_routines: eq, ne, lt, le, cneg, is_zero, is_msb_set)
- [x] Multiplexers with x86_64 cmov assembly (mux, ccopy, secret_lookup)
- [x] BearSSL constant-time division (div2n1n)
- [x] Carry/borrow arithmetic (addC, subB via widening)
- [x] Extended precision (WideMul, mul_acc, mul_double_acc)
- [x] Compiler hints (prefetch, prefetch_large)
- [x] Copy policies (CopyPolicy trait, DefaultCopyPolicy, SimdCopyPolicy)
- [x] Bit manipulation utilities (bithacks)
- [x] Platform configuration (config)
- [x] ISA assembler types (x86_64 + ARM64)
- [x] CPUID feature detection with OnceLock caching (x86_64)
- [x] RDTSC cycle counter (x86_64, moved from bench)
- [x] KperfCounter + PmuCounter (ARM64, moved from bench)
- [x] CycleCounter trait + Measurement + DefaultCounter
- [x] CachePadded (128-byte alignment)
- [x] CPU name detection
- [x] Migration: mantis-core, mantis-queue, mantis-bench updated to use platform
- [x] 162 tests, Miri validation, clippy clean

### 1.9 Fixed-Point Numeric Types (`mantis-fixed`, `mantis-types` expansion)
**Status: Complete** | Completed: 2026-04-03

- [x] `FixedI64<const D: u8>` compile-time-scaled fixed-point decimal backed by `i64`
- [x] Checked/saturating/wrapping add, sub, neg, abs
- [x] Explicit-rounding mul/div: `checked_mul_trunc`, `checked_mul_round`, `checked_div_trunc`, `checked_div_round`
- [x] Scalar integer mul/div: `checked_mul_int`, `checked_div_int`
- [x] Scale conversion: `rescale_trunc`, `rescale_round`, `checked_rescale_exact`
- [x] Display (D decimal places), Debug (raw + value), `from_str_decimal` parser
- [x] `D <= 18` compile-time bound, validated scales: 2, 4, 6, 8
- [x] `POW10_I64` const table in `mantis-platform`
- [x] Performance: hand-rolled decomposed division eliminates `__divti3` runtime call
- [x] Contender benchmarks: faster than `fixed` crate (1.10ns vs 1.20ns mul) and `rust_decimal` (4x faster add)
- [x] Domain types in `mantis-types`: `Side`, `Timestamp`, `OrderId`
- [x] Hot-path types: `Ticks(i64)`, `Lots(i64)` — pure integer, no decimal semantics
- [x] Semantic wrappers: `UsdcAmount(FixedI64<6>)`, `Probability(FixedI64<6>)`, `BtcQty(FixedI64<8>)`
- [x] `InstrumentMeta<D>` — tick/lot size conversion layer
- [x] 110 unit tests (mantis-fixed), 65 tests (mantis-types), 7 bolero property tests
- [x] Miri: 110/110 pass, zero UB
- [x] 2 fuzz targets (parse, display roundtrip)
- [x] Criterion benchmarks with contender comparison (rust_decimal, fixed crate)

### 1.3 Canonical Event Model (`mantis-events`)
**Status: Complete** | Completed: 2026-04-04

- [x] `HotEvent` — 64-byte, `Copy`, `repr(C)` envelope with header at offset 0
- [x] `EventHeader` — 24 bytes: recv_ts, seq, instrument_id, source_id, flags
- [x] `EventBody` — `repr(C, u16)` discriminated enum with 8 variants
- [x] `EventKind` — standalone `u16` discriminant with 1:1 exhaustive mapping
- [x] `EventFlags` — `u16` bitflags (IS_SNAPSHOT, LAST_IN_BATCH)
- [x] Market payloads: `BookDeltaPayload` (24B), `TradePayload` (24B), `TopOfBookPayload` (32B)
- [x] Execution payloads: `OrderAckPayload` (24B), `FillPayload` (32B), `OrderRejectPayload` (24B)
- [x] Control payloads: `TimerPayload` (8B), `HeartbeatPayload` (4B)
- [x] Supporting enums: `UpdateAction`, `OrderStatus`, `RejectReason`, `TimerKind` (all `repr(u8)`)
- [x] Constructor helpers on `HotEvent` (const fn, `#[must_use]`)
- [x] Const size assertions + authoritative layout tests in `mantis-layout`
- [x] Dependency firewall: depends on `mantis-types` only, NOT `mantis-fixed`
- [x] Prerequisites: `InstrumentId(u32)`, `SourceId(u16)` in `mantis-types`, `SeqNum` hygiene fix
- [x] 57 tests, Miri validation (57/57 pass, zero UB), no_std clean

### 1.10 Sequence Lock (`mantis-seqlock`)
**Status: Complete**

- [x] Core SeqLock<T, C> with CopyPolicy strategy pattern
- [x] Lock-free store (single-writer via &mut self)
- [x] Lock-free load (multiple readers via &self, retry on contention)
- [x] Cache-line padded sequence counter (128B alignment)
- [x] Hardware fences for ARM64 portability
- [x] Multi-threaded torn-read detection test (4 readers × 500K writes)
- [x] Layout assertions
- [x] Type aliases: SeqLockDefault, SeqLockSimd (nightly)

### 1.11 Market State Engine (`mantis-market-state`)
**Status: Complete** | Completed: 2026-04-04

- [x] `ArrayBook<N>` — fixed-size O(1) order book indexed by tick offset
- [x] `OrderBook` trait — polymorphic interface for book implementations
- [x] `MarketStateEngine<B, MAX>` — fully stack-allocated, no_std passive state machine
- [x] Snapshot state machine — IS_SNAPSHOT / LAST_IN_BATCH flags for correct BBO emission
- [x] `TopOfBook` — micro price + spread + BBO cached on price change only
- [x] `InstrumentState<B>` — per-instrument state with seq, staleness, and snapshot tracking
- [x] `Strategy` trait + `OrderIntent` + `OrderAction` — inline callback hook for strategies
- [x] Lazy query methods: `micro_price`, `book_imbalance`, `spread`, `last_trade`, `is_stale`, `is_ready`, `book`
- [x] `take_tob` — edge-triggered TopOfBook emission (clears flag on read)
- [x] TopOfBook event processed as synthetic single-level book update
- [x] Layout assertions: `TopOfBook` fits in one 64B cache line; `ArrayBook<100>` size verified
- [x] Criterion benchmarks: `apply_delta`, `best_bid`, `process_delta` (mid/end), `micro_price`, `book_imbalance`
- [x] Bolero property tests: random deltas no panic, best_bid invariant, imbalance range
- [x] 17 unit tests, no_std clean, layout assertions in `mantis-layout`

### 1.12 Strategy Runtime (`mantis-strategy`)
**Status: Complete** | Completed: 2026-04-06

- [x] Event-driven `Strategy` trait — no generics, associated consts (`STRATEGY_ID`, `NAME`)
- [x] `OrderIntent` + `OrderAction` (Post/Cancel/Amend) with `target_order_id` disambiguation
- [x] `SignedLots(i64)` newtype in `mantis-types` for signed inventory positions
- [x] `Position` — signed qty (`SignedLots`), VWAP entry, realized/unrealized PnL, flat invariant
- [x] `OrderTracker` — fixed-size `[Option<TrackedOrder>; 64]` order state machine with slot reclamation
- [x] `QueueEstimator` — L2 probabilistic queue model (`PowerProbQueueFunc`, per-instrument take rates)
- [x] `ExposureView` — composes position + open orders for worst-case risk calculation
- [x] `RiskLimits` + `RiskCheckResult` — per-strategy risk configuration
- [x] `StrategyContext<B, MAX>` — optional helper bundle (engine + queue + orders + risk + positions)
- [x] Old `Strategy` trait removed from `mantis-market-state` (clean break, no deprecation)
- [x] 38 unit tests, `no_std` clean, clippy clean, 3 rounds of Codex review

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
| `mantis-core` | Active | yes | 1 | — | — |
| `mantis-types` | Active | yes | 98 | — | — |
| `mantis-fixed` | Active | yes | 110 | 7 groups + 2 contenders | miri pass, 7 bolero props, 2 fuzz |
| `mantis-events` | Active | yes | 62 | — | miri pass |
| `mantis-queue` | Active | yes | 31 | — | miri pass |
| `mantis-platform` | Active | yes | 164 | — | miri pass |
| `mantis-seqlock` | Active | yes | 1 | — | miri pass |
| `mantis-market-state` | Active | yes | 17 | 6 criterion groups | 3 bolero props |
| `mantis-bench` | Active | std | 11 | 6+7 bench groups, 6 contenders | — |
| `mantis-layout` | Active | std | 6 | — | — |
| `mantis-verify` | Active | std | 13 | — | 4 kani proofs, 13 bolero/diff |
| `mantis-strategy` | Active | yes | 38 | — | 3 rounds Codex review |
