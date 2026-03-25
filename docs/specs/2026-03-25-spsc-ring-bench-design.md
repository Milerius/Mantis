# SPSC Ring Buffer + Benchmark Harness — Design Spec

## Goal

Implement a high-performance, lock-free SPSC ring buffer in `mantis-queue` with a complete benchmark harness in `mantis-bench`, platform-specific cycle counters, external contender comparisons, formal verification, and CI improvements for published results and regression tracking.

## Architecture

The SPSC ring follows the modular strategy pattern established in Phase 0. A single generic engine is parameterized by strategy traits. Curated preset aliases hide the generics. A safe split-handle API enforces the SPSC contract at compile time. All unsafe code is isolated in a `raw` submodule.

The benchmark harness extends `mantis-bench` with platform-aware cycle counters, criterion integration, standardized workload shapes, and FFI-based external contender benchmarks. A Godbolt ASM inspection script provides assembly-level validation.

## Tech Stack

- Rust (no_std core, std for bench/verify)
- `core::sync::atomic` (Acquire/Release + cached indices)
- `core::arch::asm!` (RDTSC on x86_64, behind `asm` feature)
- Criterion 0.5 (statistical benchmarking)
- Bolero (property-based testing)
- Kani (bounded model checking)
- `cc` crate (C++ FFI build for contenders)

---

## 1. SPSC Ring Engine

### 1.1 Core Struct

```rust
pub(crate) struct RingEngine<T, S, I, P, Instr>
where
    S: Storage<T>,
    I: IndexStrategy,
    P: PushPolicy,
    Instr: Instrumentation,
{
    head: CachePadded<AtomicUsize>,        // producer writes
    tail: CachePadded<AtomicUsize>,        // consumer writes
    tail_cached: CachePadded<Cell<usize>>, // producer's local cache of tail
    head_cached: CachePadded<Cell<usize>>, // consumer's local cache of head
    storage: S,
    _marker: PhantomData<(T, I, P, Instr)>,
}
```

`CachePadded<T>` is a `#[repr(align(128))]` wrapper defined in `mantis-queue` (not an external dependency). Uses 128-byte alignment to cover both Intel (64B) and Apple Silicon (128B) cache lines. Zero external deps in the hot-path core.

Cache padding on all atomic and cached fields prevents false sharing. `Cell<usize>` for cached indices is safe because each cached field is only accessed by its owning side (producer or consumer).

Note: `RingEngine` is `!Sync` by design (due to `Cell<usize>`). The split `Producer`/`Consumer` handles require `unsafe impl Send` with a documented safety argument: each handle exclusively accesses its own side of the engine (producer writes `head`/`tail_cached`, consumer writes `tail`/`head_cached`). This unsafe impl lives in the `raw` module per `docs/UNSAFE.md` and is validated by miri's data-race detection.

### 1.2 Memory Ordering

Acquire/Release with cached remote indices (Rigtorp pattern):

**Push (producer):**
1. Load own `head` with `Relaxed` (sole writer)
2. Compute `next_head` via `IndexStrategy::wrap`
3. If `next_head == tail_cached`: reload `tail` with `Acquire`, update cache. If still full, return `Err`
4. Write slot via `raw::slot::write(storage, head, value)`
5. Store `head` with `Release`
6. Call `Instr::on_push()`

**Pop (consumer):**
1. Load own `tail` with `Relaxed` (sole writer)
2. If `tail == head_cached`: reload `head` with `Acquire`, update cache. If still empty, return `Err`
3. Read slot via `raw::slot::read(storage, tail)`
4. Compute `next_tail` via `IndexStrategy::wrap`
5. Store `tail` with `Release`
6. Call `Instr::on_pop()`

In the common case (queue not full/empty), the hot path avoids cross-core atomic loads entirely. On x86_64, Acquire/Release compiles to plain `mov` instructions (no extra barriers). On ARM64, it uses `ldar`/`stlr` which are the minimal correct instructions.

### 1.3 Element Types

Both `T: Copy` and `T: Send` are supported through the same engine:

- Slots are `MaybeUninit<T>` — no unnecessary initialization
- `raw::slot::write` uses `ptr::write` (works for both Copy and move types)
- `raw::slot::read` uses `ptr::read` (returns owned `T`, works for both)
- For `T: !Copy`, drop safety is handled by the slot module: a slot is either uninitialized or contains a valid `T`, never both. `write` fills an empty slot, `read` empties a full slot.

### 1.6 Drop Safety

When the ring is dropped with un-popped elements of type `T: Drop` (e.g., `String`), those elements must be properly dropped. The `Drop` impl for the shared ring state:

1. Loads `head` and `tail` (no ordering needed — we're the sole owner during drop)
2. Iterates occupied slots from `tail..head` (wrapping via `IndexStrategy::wrap`)
3. Calls `raw::slot::drop_in_place(storage, index)` on each occupied slot
4. This is unsafe code and lives in the `raw` module with a `// SAFETY:` comment

For `T: Copy`, the `Drop` impl is a no-op (the compiler optimizes it away).

### 1.4 Storage Trait

```rust
pub trait Storage<T>: Send + Sync {
    /// Number of usable slots.
    fn capacity(&self) -> usize;

    /// Pointer to the slot at `index`. Caller must ensure index < capacity.
    unsafe fn slot_ptr(&self, index: usize) -> *mut MaybeUninit<T>;
}
```

**Implementations:**

- `InlineStorage<T, const N: usize>` — `[UnsafeCell<MaybeUninit<T>>; N]`. True no_std, const-generic. `N` must be a power of 2 (enforced at compile time via `AssertPowerOfTwo`).
- `HeapStorage<T>` — `Box<[UnsafeCell<MaybeUninit<T>>]>`. Runtime-sized, requires `alloc`. Capacity rounded up to next power of 2 at construction.

### 1.5 Unsafe Isolation

All unsafe code lives in `crates/queue/src/raw/`:

- `raw::slot` — `write(storage, index, value)`, `read(storage, index) -> T`, `drop_in_place(storage, index)`
- Every `unsafe` block has a `// SAFETY:` comment documenting invariant, guarantee, and failure mode
- The `raw` module uses `#[allow(unsafe_code)]`; the crate root keeps `#![deny(unsafe_code)]`
- The engine module (`engine.rs`) calls into `raw` but contains no `unsafe` itself

---

## 2. Public API

### 2.1 Split Handles

```rust
/// Create an inline-storage ring, split into producer/consumer.
pub fn spsc_ring<T, const N: usize>() -> (Producer<T, InlineStorage<T, N>, ...>, Consumer<T, InlineStorage<T, N>, ...>)

/// Create a heap-storage ring, split into producer/consumer.
pub fn spsc_ring_heap<T>(capacity: usize) -> (Producer<T, HeapStorage<T>, ...>, Consumer<T, HeapStorage<T>, ...>)
```

**Producer:**
- `try_push(value: T) -> Result<(), PushError<T>>` — returns `PushError::Full(value)` on full, preserving the value for retry
- `try_push_batch(items: &[T]) -> usize` — batch push, returns count written (T: Copy)

**Consumer:**
- `try_pop() -> Result<T, QueueError>` — returns `QueueError::Empty` on empty
- `try_pop_batch(buf: &mut [T]) -> usize` — batch pop, returns count read (T: Copy)

**Error types** (in `mantis-types`):
- `PushError<T>` — `PushError::Full(T)` wraps both the error kind and the returned value. This is distinct from `QueueError::Full` (which remains available for contexts where the value isn't returned, e.g., instrumentation, logging).
- `QueueError::Empty` — used by pop, no value to return.

Both handles are `Send` but not `Sync`. The underlying `RingEngine` is wrapped in an `Arc` (with `alloc`) or a shared reference (for inline/static use).

### 2.2 Preset Type Aliases

| Preset | Storage | Index | Push | Instr | Use case |
|---|---|---|---|---|---|
| Preset | Storage | Index | Push | Padding | Instr | Use case |
|---|---|---|---|---|---|---|
| `SpscRingPortable<T, N>` | Inline | Pow2Masked | ImmediatePush | No | NoInstr | Minimal baseline, no padding overhead |
| `SpscRingPadded<T, N>` | Inline | Pow2Masked | ImmediatePush | Yes | NoInstr | Cache-padded (recommended default) |
| `SpscRingHeap<T>` | Heap | Pow2Masked | ImmediatePush | Yes | NoInstr | Runtime-sized |
| `SpscRingInstrumented<T, N>` | Inline | Pow2Masked | ImmediatePush | Yes | CountingInstr | Debug/profiling |

`SpscRingPortable` uses a compact engine variant where head/tail atomics are **not** wrapped in `CachePadded`. This saves ~240 bytes of padding overhead and is suitable for very small buffers or single-threaded use (replay, testing). It serves as the differential testing baseline.

`SpscRingPadded` is the recommended default for multi-threaded use. The padding is controlled by a const-generic bool or a `PaddingStrategy` marker type on the engine.

**`CountingInstr`** (added to `mantis-core`):
```rust
pub struct CountingInstr {
    pushes: AtomicU64,
    pops: AtomicU64,
    push_full: AtomicU64,
    pop_empty: AtomicU64,
}
```
Each `on_*` method increments the corresponding counter with `Relaxed` ordering. Accessor methods (`push_count()`, etc.) read with `Relaxed`. Lives in `mantis-core`, `no_std` compatible via `core::sync::atomic`.

### 2.3 Raw Access

```rust
pub mod raw {
    /// Direct ring access without split handles.
    /// For single-threaded replay, benchmarking, and power users.
    pub struct RawRing<T, S, I, P, Instr> { ... }
}
```

The raw ring exposes `push`/`pop` directly on the struct. No handle splitting, no Arc. Useful for single-threaded scenarios (replay, unit tests) and for benchmarking without handle overhead.

---

## 3. Benchmark Harness

### 3.1 Platform Cycle Counters

| Platform | Struct | Method | Feature gate |
|---|---|---|---|
| x86_64 | `RdtscCounter` | `lfence; rdtsc; lfence` inline asm | `asm` feature |
| macOS ARM64 | `KperfCounter` | `mach_absolute_time()` for nanos | default |
| Linux ARM64 | `PmuCounter` | `clock_gettime` for nanos, `perf_event_open` for cycles | default |
| Fallback | `InstantCounter` | `std::time::Instant` | always available |

Counter selection is compile-time via `cfg(target_arch)` + `cfg(target_os)`. No runtime dispatch. When `asm` feature is disabled on x86_64, falls back to `InstantCounter`.

The RDTSC counter follows the Constantine model:
- `lfence` before `rdtsc` to serialize (prevent out-of-order reads)
- `lfence` after to prevent speculative reads past the measurement point
- `#[inline(never)]` on the benchmark wrapper to prevent optimizer interference
- Warmup phase to stabilize CPU frequency before measurement

### 3.2 Criterion Integration

`mantis-bench` implements criterion's `Measurement` trait backed by the platform counter:

`MantisMeasurement` is generic over the counter to avoid runtime dispatch:

```rust
pub struct MantisMeasurement<C: CycleCounter> {
    counter: C,
}

impl<C: CycleCounter> criterion::measurement::Measurement for MantisMeasurement<C> {
    // Returns both nanos and cycles
}
```

A `cfg`-gated type alias selects the concrete counter at compile time:
```rust
#[cfg(all(target_arch = "x86_64", feature = "asm"))]
pub type DefaultMeasurement = MantisMeasurement<RdtscCounter>;

#[cfg(not(all(target_arch = "x86_64", feature = "asm")))]
pub type DefaultMeasurement = MantisMeasurement<InstantCounter>;
```

This allows criterion benchmarks to report cycles alongside wall-clock time with zero dynamic dispatch overhead.

### 3.3 Workload Shapes

All workloads are defined in `mantis-bench/src/workloads.rs` and used identically for all implementations:

| Shape | Description |
|---|---|
| `single_item` | Push 1, pop 1, repeat N times. Measures per-op latency. |
| `burst_100` | Push 100, pop 100, repeat. Measures batch amortization. |
| `burst_1000` | Push 1000, pop 1000, repeat. Larger batch. |
| `ping_pong` | Two threads: producer pushes continuously, consumer pops. Measures sustained throughput + tail latency. |
| `full_drain` | Fill ring completely, then drain completely. Measures worst-case cache behavior. |

Element types tested: `u64` (8 bytes), `[u8; 64]` (cache-line sized), `[u8; 256]` (large payload).

### 3.4 External Contenders

Behind `bench-contenders` feature flag in `mantis-bench`:

| Contender | Language | Integration |
|---|---|---|
| `rtrb` 0.3 | Rust | Cargo dependency |
| `crossbeam::ArrayQueue` | Rust | Cargo dependency |
| Rigtorp `SPSCQueue.h` | C++ | FFI via `cc`, thin C wrapper in `ffi/` |
| Drogalis `SPSC-Queue` | C++ | FFI via `cc`, thin C wrapper in `ffi/` |

C++ sources are vendored in `crates/bench/ffi/vendor/` (header-only libraries). The `cc` build script compiles thin C wrappers that expose `create`, `push`, `pop`, `destroy` functions. Rust calls these via `extern "C"`.

All contenders run the exact same workload shapes with the same element types. Results are exported to JSON.

### 3.5 BenchReport

Extend the existing `BenchReport` struct:

```rust
pub struct BenchReport {
    pub implementation: String,
    pub arch: String,
    pub os: String,
    pub cpu: String,
    pub compiler: String,
    pub features: Vec<String>,       // ["asm", "bench-contenders", ...]
    pub results: Vec<WorkloadResult>,
}

pub struct WorkloadResult {
    pub workload: String,
    pub element_type: String,
    pub ops_per_sec: f64,
    pub ns_per_op: f64,
    pub cycles_per_op: Option<f64>,
    pub p50_ns: f64,
    pub p99_ns: f64,
    pub p999_ns: f64,
}
```

Export to JSON via serde. Cross-run comparison script reads multiple JSON files and produces a comparison matrix.

### 3.6 Godbolt ASM Inspection

`scripts/check-asm.sh`:

1. Extracts hot function source (push/pop) from Mantis presets
2. Sends to Godbolt Compiler Explorer API for x86_64 (`-C opt-level=3 -C target-cpu=native`) and aarch64 (`-C opt-level=3`)
3. Saves output to `target/asm/{function}_{arch}.s`
4. If a baseline exists in `target/asm/baseline/`, diffs against it and reports instruction count changes
5. Nightly CI job runs this and archives the output as an artifact

The primary local approach is `cargo-show-asm` for quick inspection:
```
cargo asm --lib -p mantis-queue "mantis_queue::engine::RingEngine::push"
```
The Godbolt script is a secondary tool for cross-architecture comparison and CI regression detection.

---

## 4. Verification

### 4.1 Unit Tests (crates/queue)

- Push/pop single item on each preset
- Fill to capacity, verify full, pop all, verify empty
- Wraparound: push N+1 items (with pops between), verify sequence
- Batch push/pop correctness
- `T: Copy` (u64) and `T: Send` (String) element types
- All storage variants: Inline, Heap
- All presets: Portable, Padded, Heap, Instrumented
- Error conditions: push when full returns `Err`, pop when empty returns `Err`

### 4.2 Miri

Run on all queue tests including two-thread stress test:
```
cargo +nightly miri test -p mantis-queue --all-features
```
Flags: `-Zmiri-strict-provenance -Zmiri-symbolic-alignment-check`

### 4.3 Careful

```
cargo +nightly careful test -p mantis-queue --all-features
```

### 4.4 Kani Proofs (crates/verify)

Bounded model checking proofs:
- **FIFO ordering**: For all push/pop sequences of length <=8 on capacity 4, output order matches input order
- **No data loss**: Items pushed are always retrievable (no silent drops)
- **Capacity invariant**: Ring never accepts more than `capacity` items without a pop
- **Index safety**: Wrapped indices never exceed storage bounds

### 4.5 Differential Testing

Custom test in `mantis-verify`:
- Generate random push/pop command sequences (via bolero)
- Execute the same sequence on all presets: Portable, Padded, Heap, Instrumented
- Assert all produce identical output sequences
- Also compare against `rtrb` as external oracle

### 4.6 Mutation Testing

Nightly CI runs `cargo-mutants` targeting the `raw` module:
- Mutants in slot operations (write/read) must be killed by existing tests
- Mutants in memory ordering (Acquire -> Relaxed) should be caught by miri/thread tests

### 4.7 Property-Based Tests (Bolero)

In `mantis-verify/src/spsc_props.rs`:
- Arbitrary push/pop sequences maintain FIFO ordering
- `count_pushed - count_popped == ring.len()` invariant holds
- Ring never reports full when len < capacity
- Ring never reports empty when len > 0
- Two-thread interleaving maintains data integrity

### 4.8 Thread Stress Test

In `mantis-queue` integration tests:
- Producer thread pushes 10M sequential u64 values
- Consumer thread pops and verifies monotonic ordering
- Run under miri (with reduced count) to verify no data races
- Run with all presets

---

## 5. ASM Feature Toggle (Constantine Model)

### 5.1 mantis-queue

`asm` feature flag in `Cargo.toml`. When enabled:
- Future platform-specific atomic store paths (x86_64 `lock` prefix, ARM64 `stlr`/`ldar`) can be used
- Currently no asm in the queue — the feature is a forward-compatible toggle
- CI tests both `--features asm` and without

### 5.2 mantis-bench

`asm` feature flag. When enabled:
- x86_64: `RdtscCounter` uses `lfence; rdtsc; lfence` inline asm
- When disabled: falls back to `InstantCounter`
- CI tests both paths

### 5.3 CI Matrix

| Job | Features | Purpose |
|---|---|---|
| test | default | Portable baseline |
| test | `asm` | ASM-enabled path |
| test | `--no-default-features` | no_std validation |
| bench | default + `bench-contenders` | Full comparison |
| bench | `asm` + `bench-contenders` | ASM-enabled comparison |

---

## 6. CI Improvements

### 6.1 Benchmark Dashboard

Use `benchmark-action/github-action-benchmark` in the bench workflow:
- Store historical data in `gh-pages` branch
- Auto-update on every merge to main
- Show ops/sec, ns/op, cycles across all presets + contenders
- PR comment with regression/improvement % vs main
- Fail PR if any benchmark regresses >5% (threshold may need tuning; CI runners with shared hardware can produce noisy results — consider requiring N consecutive regressions or statistical significance testing. Bare-metal/dedicated runners are preferred for benchmark jobs.)
- Publicly visible dashboard URL in README

### 6.2 Coverage Reporting

Enhance the coverage job:
- Upload to Codecov or similar service for historical tracking
- PR comment with coverage delta
- Badge in README showing current coverage %
- Fail PR if coverage drops below configured threshold

### 6.3 Test Results

Add `dorny/test-reporter` action to the test job:
- Publishes test results as GitHub check annotations
- Shows test count, pass/fail breakdown in PR checks
- Makes individual test failures visible without reading logs

### 6.4 Kani Proof Reporting

After kani runs:
- Parse output for proof count + verification time
- Add step summary with proof results
- Archive proof output as artifact

### 6.5 Fuzzing Artifacts

In the nightly fuzz workflow:
- Upload corpus as artifact for persistence across runs
- If crashes found, upload crash inputs as separate artifact
- Add step summary with fuzz duration + corpus size

### 6.6 ASM Toggle in CI

Add ASM feature toggle to PR gate:
- Test with `--features asm` on x86_64 runners
- Test without `asm` (fallback) on all runners
- Both must pass before merge

### 6.7 Godbolt ASM in Nightly

Add nightly job:
- Run `scripts/check-asm.sh`
- Archive ASM output as artifact
- Diff against previous run's artifact
- If instruction count changes by >10%, flag in step summary

---

## 7. File Layout

```
crates/queue/
├── Cargo.toml                # Add asm feature
├── src/
│   ├── lib.rs                # Public API, presets, re-exports
│   ├── engine.rs             # RingEngine<T, S, I, P, Instr>
│   ├── handle.rs             # Producer<T> / Consumer<T>
│   ├── storage.rs            # Storage trait, InlineStorage, HeapStorage
│   └── raw/
│       ├── mod.rs            # #[allow(unsafe_code)]
│       └── slot.rs           # ptr::write/read, MaybeUninit ops
├── tests/
│   └── spsc_stress.rs        # Two-thread 10M item stress test

crates/bench/
├── Cargo.toml                # Add asm, bench-contenders features
├── src/
│   ├── lib.rs
│   ├── counters.rs           # CycleCounter trait, counter selection
│   ├── counters/
│   │   ├── rdtsc.rs          # x86_64 lfence+rdtsc (asm feature)
│   │   ├── kperf.rs          # macOS ARM64
│   │   └── pmu.rs            # Linux ARM64
│   ├── report.rs             # BenchReport (extended)
│   ├── measurement.rs        # Criterion Measurement impl
│   └── workloads.rs          # Workload shapes
├── benches/
│   ├── spsc_mantis.rs        # All Mantis presets
│   └── spsc_contenders.rs    # rtrb/crossbeam/Rigtorp/Drogalis
├── build.rs                  # cc build for C++ contenders (Cargo requires crate root)
└── ffi/
    ├── rigtorp_wrapper.cpp
    ├── drogalis_wrapper.cpp
    └── vendor/               # Vendored C++ headers
        ├── SPSCQueue.h       # Rigtorp
        └── SPSCQueue.hpp     # Drogalis

crates/core/
├── src/
│   └── lib.rs                # Add CountingInstr

crates/verify/
├── src/
│   ├── lib.rs
│   ├── spsc_props.rs         # Bolero property tests
│   └── spsc_proofs.rs        # Kani proofs

scripts/
└── check-asm.sh              # Godbolt API inspection

.github/workflows/
├── ci.yml                    # Add ASM toggle, test-reporter
├── bench.yml                 # Add benchmark-action, contenders
└── nightly.yml               # Add godbolt, fuzz corpus upload
```

---

## 8. Constantine Alignment

| Constantine Pattern | Mantis Implementation |
|---|---|
| Compile-time CPU detection via cfg | Counter selection + atomic paths, no runtime dispatch |
| Inline asm in dedicated modules | `counters/rdtsc.rs`, future `raw/asm/` |
| ASM toggle flag, CI tests both | `asm` feature, CI matrix with/without |
| Differential testing vs reference | All presets + rtrb as external oracle |
| Zero external deps in hot-path core | `mantis-queue` depends only on `mantis-core` + `mantis-types` |
| noinline + volatile for bench | `black_box`, `#[inline(never)]` on bench wrappers |
| Warmup + CPU freq stabilization | Criterion warmup + explicit warmup in counter layer |
| Report: CPU, compiler, ops/sec, ns/op, cycles | `BenchReport` with JSON export |

---

## 9. Deferred Scope

The following are explicitly out of scope for this spec:

- **Batch commit optimization** (write N items, publish head once) — add after initial benchmarks show it matters
- **Huge pages** for ring buffer memory — OS-specific, add later
- **Wait/notify** (futex-based blocking when empty) — add as a `PushPolicy`/`PopPolicy` strategy later. Note: the current `PushPolicy` trait (`fn should_block() -> bool`) is too narrow for this; it will need to be redesigned when blocking/spinning policies are added. Similarly, a `PopPolicy` trait does not yet exist and adding one will require a new generic parameter on `RingEngine` (breaking change to presets).
- **MPSC/MPMC queues** — separate primitive, separate spec
- **Platform-specific atomic asm in queue** — the `asm` feature is a forward-compatible toggle; actual asm paths added when benchmarks show Rust atomics aren't optimal
