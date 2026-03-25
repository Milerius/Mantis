# Mantis Repository Bootstrap — Design Spec

**Date:** 2026-03-25
**Status:** Approved
**Scope:** Infrastructure bootstrap — full CI, tooling, workspace setup before SPSC ring implementation

> **Scope note:** The blueprint (`philosophy/fin_sdk_oss_blueprint.md`) defines a broader Phase 0 that includes domain crate skeletons (event, book, capture, replay, cli, examples). This spec covers **infrastructure-only bootstrap** — the 6 crates here are the minimum needed to build, test, benchmark, verify, and inspect the first primitive (SPSC ring). Domain crates are added as implementation phases begin. TUI (mentioned in the modular strategy doc as v0.1 tooling) is deferred to a later phase.

## 1. Overview

Set up the Mantis repository as a fully agent-friendly, CI-hardened Rust workspace before writing any domain code. Every commit from day one goes through the full quality gauntlet. Infrastructure before implementation — tooling is part of the architecture.

### Naming
The workspace uses `mantis-*` crate names (e.g., `mantis-core`, `mantis-queue`), superseding the `fin_*` working names from the blueprint. "Mantis" is the project identity.

### License
Apache-2.0 (confirmed). Permissive, patent grant, corporate-friendly.

### Reference Architecture
- Constantine (https://github.com/mratsim/constantine): primary mental model for platform-specific code, benchmark architecture, optimization separation, and coding discipline.

## 2. Workspace Structure

```
Mantis/
├── .claude/
│   ├── memory/                      # Agent reference docs
│   │   └── constantine-reference.md
│   └── settings.local.json
├── .github/
│   ├── dependabot.yml               # Dependency updates (7-day cooldown, grouped)
│   └── workflows/
│       ├── ci.yml                   # PR gate
│       ├── bench.yml                # Benchmark regression
│       ├── nightly.yml              # Extended checks
│       └── verify.yml               # Formal verification
├── crates/
│   ├── core/                        # mantis-core (no_std)
│   ├── types/                       # mantis-types (no_std)
│   ├── queue/                       # mantis-queue (no_std)
│   ├── bench/                       # mantis-bench (std)
│   ├── layout/                      # mantis-layout (std)
│   └── verify/                      # mantis-verify (std)
├── benches/                         # Criterion benchmark binaries (depend on mantis-bench for harness)
│   ├── contenders/                  # FFI wrappers for external impls
│   │   ├── rtrb/
│   │   ├── ringbuf/
│   │   └── folly_pcq/              # C++ FFI
│   └── spsc_ring.rs                 # Comparative benchmarks
├── fuzz/                            # cargo-fuzz targets
├── philosophy/                      # Existing design docs
├── docs/
│   ├── specs/                       # Design specs (this file)
│   └── UNSAFE.md                    # Unsafe policy
├── Cargo.toml                       # Workspace root
├── deny.toml                        # cargo-deny config
├── rust-toolchain.toml              # Toolchain pin
├── clippy.toml                      # Clippy config
├── justfile                         # Task runner (tool install, common commands)
├── CLAUDE.md                        # Agent instructions
├── LICENSE                          # Apache 2.0
└── README.md
```

### Crate Summary

| Crate | no_std | Purpose |
|---|---|---|
| mantis-core | yes | Traits, foundations, strategy trait definitions |
| mantis-types | yes | IDs, newtypes, error types, capacity markers |
| mantis-queue | yes | SPSC ring + future queue primitives |
| mantis-bench | no | Criterion + custom perf-counter harness |
| mantis-layout | no | Struct layout / cache-line inspector |
| mantis-verify | no | Kani proofs, bolero property tests |

### Initial Type Inventory

**mantis-core:**
- `PushPolicy` trait — defines push behavior when queue is full
- `Instrumentation` trait — defines measurement hooks
- `IndexStrategy` trait — defines how head/tail indices wrap
- Strategy marker types: `ImmediatePush`, `NoInstr`, `Pow2Masked`

**mantis-types:**
- `Capacity<const N: usize>` — compile-time power-of-2 capacity marker
- `QueueError` — error enum (`Full`, `Empty`)
- `SeqNum(u64)` — sequence number newtype
- `SlotIndex(usize)` — slot index newtype

These grow as implementation reveals needs. The above is the minimum for the SPSC ring.

### no_std Rules
- core, types, queue: `#![no_std]` by default, optional `std` feature
- No heap allocation in hot paths after init
- No panics in hot paths — `Result` or error enum returns
- bench, layout, verify: `std`-only tooling crates, never depended on by core

### no_std Testing Approach
The `no_std` crates use `#![no_std]` in `src/lib.rs` but the test harness requires `std`. This is handled by:
- `dev-dependencies` implicitly enable `std` for the test binary
- `#[cfg(test)]` modules can use `std` freely — they don't ship
- The `test-no-std` CI job runs `cargo test --no-default-features` which verifies the library code compiles as `no_std`, while the test binary itself links `std` for the harness
- This is the standard Rust pattern used by `heapless`, `embedded-hal`, and other `no_std` crates

## 3. CI Architecture

### 3.1 `ci.yml` — PR Gate (every PR + push to main)

**Platforms:** Linux x86_64 + macOS ARM64

| Job | Toolchain | Description |
|---|---|---|
| fmt | stable | `cargo fmt --all --check` |
| clippy | stable | `cargo clippy --all-targets --all-features -- -D warnings` |
| test | stable | `cargo test --all-features` |
| test-no-std | stable | `cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features` |
| doc | stable | `cargo doc --no-deps --all-features` |
| deny | stable | `cargo deny check` |
| miri | nightly | `cargo +nightly miri test -p mantis-queue` |
| careful | nightly | `cargo +nightly careful test` |
| coverage | stable | `cargo llvm-cov --codecov` + diff coverage as PR comment |
| coverage-branch | stable | `cargo llvm-cov --branch` |

### 3.2 `bench.yml` — Benchmark Regression (every PR)

**Platform:** Linux x86_64

| Job | Description |
|---|---|
| bench-baseline | Checkout main, run criterion, save baseline |
| bench-candidate | Checkout PR, run criterion, compare |
| bench-contenders | Run with `--features bench-contenders` |
| bench-report | JSON/markdown report, post as PR comment |

Thresholds: warn at 5% degradation, fail at 10%. Configurable per group.

### 3.3 `nightly.yml` — Extended Checks (scheduled + manual)

**Platforms:** Linux x86_64, Linux ARM64, macOS ARM64

| Job | Toolchain | Description |
|---|---|---|
| mutants | stable | `cargo mutants --timeout 60` |
| fuzz | nightly | `cargo +nightly fuzz run` each target, 60s |
| miri-extended | nightly | Extended iterations, stacked + tree borrows |
| asm-check | stable | Build + test with ASM enabled and disabled |
| bench-matrix | stable | Full matrix: impls x payloads x capacities, JSON export |
| coverage-full | stable | `cargo llvm-cov --html` artifact |
| fuzz-coverage | nightly | Coverage of fuzz corpus |

### 3.4 `verify.yml` — Formal Verification (nightly + manual)

**Platform:** Linux x86_64

| Job | Description |
|---|---|
| kani | Bounded model checking for unsafe invariants |
| bolero | Extended property-based test campaigns |

### CI Matrix Summary

| Check | PR | Nightly | Platforms |
|---|---|---|---|
| fmt, clippy, doc | yes | — | Linux x86_64, macOS ARM64 |
| test (std + no_std) | yes | — | Linux x86_64, macOS ARM64 |
| deny | yes | — | Linux x86_64 |
| miri | yes | extended | Linux x86_64 |
| careful | yes | — | Linux x86_64 |
| diff coverage | yes | — | Linux x86_64 |
| full coverage + branch | — | yes | Linux x86_64 |
| bench regression | yes | full matrix | Linux x86_64 |
| mutants | — | yes | Linux x86_64 |
| fuzz | — | yes | Linux x86_64 |
| asm on/off | — | yes | Linux x86_64, ARM64, macOS ARM64 |
| kani proofs | — | yes | Linux x86_64 |

## 4. Coding Model (Constantine-Inspired)

### 4.1 Module Structure for Optimized Primitives

Every performance-critical module follows this layout:

```
crates/queue/src/spsc/
├── mod.rs              # Public facade — re-exports presets only
├── contract.rs         # Semantic contract traits
├── engine.rs           # Generic internal engine
├── portable.rs         # Portable baseline (always available)
├── strategies/
│   ├── mod.rs
│   ├── index.rs        # Pow2Masked, ModuloIndex
│   ├── publish.rs      # ImmediatePublish, BatchPublish
│   ├── padding.rs      # NoPadding, CacheLinePadded (contention/layout)
│   └── instrumentation.rs  # NoInstr, CounterInstr
├── assembly/           # Platform-specific fast paths
│   ├── mod.rs          # cfg-gated imports
│   ├── x86_64.rs
│   └── aarch64.rs
├── presets.rs          # Curated type aliases
└── raw/                # ALL unsafe code lives here
    ├── mod.rs          # #![allow(unsafe_code)]
    ├── slot.rs         # MaybeUninit slot management
    └── atomic.rs       # Atomic index operations
```

### 4.2 Dispatch Pattern

Portable is default. Platform paths are cfg-gated — no runtime dispatch in hot paths:

```rust
#[inline(always)]
pub(crate) fn load_head(head: &AtomicUsize) -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: x86_64 TSO guarantees acquire semantics on all loads;
        // Relaxed is sufficient here as the hardware enforces ordering.
        head.load(Ordering::Relaxed)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        head.load(Ordering::Acquire)
    }
}
```

### 4.3 Naming Conventions

| Pattern | Convention | Example |
|---|---|---|
| Constant-time by default | no suffix | `push`, `pop` |
| Variable-time variant | `_vartime` suffix | `push_vartime` |
| Platform-specific internal | `_x86_64` / `_aarch64` | `store_head_x86_64` |
| Conditional operations | `c` prefix | `ccopy`, `cswap` |
| Unsafe internals | in `raw` module | `raw::slot::write_unchecked` |
| Public preset types | descriptive | `SpscRingPortable`, `SpscRingPadded` |
| Internal engine | generic params | `SpscRing<T, N, Idx, Pub, Instr>` |

### 4.4 Layer Discipline

```
Layer 0: raw/          Unsafe primitives (atomics, MaybeUninit, asm)
Layer 1: strategies/   Strategy trait impls (safe code over raw)
Layer 2: engine.rs     Generic engine composing strategies (safe, no_std)
Layer 3: presets.rs    Curated type aliases (public API)
Layer 4: mod.rs        Public facade — re-exports presets + contract traits
```

Consumers see Layer 4 only. Layers 0-2 are `pub(crate)`.

### 4.5 Documentation Style

Every public type documents:
- Semantic contract (push/pop behavior, ordering, capacity)
- Memory ordering rationale
- Algorithm description with pseudocode
- Performance target and benchmark pointer
- Layout inspection command

### 4.6 Priority Order

```
1. Correctness (kani, miri, differential tests)
2. Safety (unsafe isolated, SAFETY comments, no UB)
3. Performance (benchmarked, layout-inspected, asm-verified)
4. Code size / stack usage
5. Ergonomics
```

## 5. Benchmark Architecture

### 5.1 mantis-bench Crate

- Criterion for statistical rigor and regression detection
- Custom perf-counter layer:
  - Linux: `perf_event_open` syscall (cycles, instructions, cache misses, branch misses)
  - macOS: kperf framework when available (requires `sudo` or signed binary with entitlements). Fallback: `mach_absolute_time`-based timing when kperf is unavailable (CI, unprivileged environments).
- Core measurement types are `no_std`-compatible for hot-path instrumentation
- JSON/CSV export for cross-hardware comparison
- Warmup phase to stabilize CPU frequency (Constantine pattern)

### 5.1.1 Benchmark Regression Details
- Metric: wall-clock time via criterion's built-in statistical comparison
- Baseline: stored as CI artifact on main branch, updated on merge
- Minimum sample: criterion default (100 iterations minimum, auto-tuned)
- Thresholds: warn at 5% degradation, fail at 10% (configurable per benchmark group via `criterion.toml`)

### 5.2 Comparative Benchmarks

External contenders via FFI behind `bench-contenders` feature:
- **Rust contenders** (dev-dependencies): `rtrb`, `ringbuf`, `crossbeam`, `heapless::spsc`
- **C++ contenders** (cc crate FFI): Folly ProducerConsumerQueue, DPDK ring

All contenders go through the same measurement path via a `Contender` trait.

### 5.3 Benchmark Matrix

Dimensions: **implementations x workloads x payloads x capacities x hardware x contenders**

Workload shapes: steady state, burst producer, near-full, near-empty, skewed rates.

## 6. Formal Verification

### 6.1 Kani (Bounded Model Checking)

Installed via `cargo install --locked kani-verifier && cargo kani setup`. CI uses the `model-checking/kani-verifier-action` GitHub Action with a pinned version. Version tracked in `justfile`.

Proof harnesses in `crates/verify/` for:
- SPSC ring: no data race, no double-read, bounded capacity never exceeded
- Atomic operations: memory ordering correctness for all interleavings
- `unsafe impl Send/Sync`: formal argument backing

### 6.2 Bolero (Property-Based Testing)

Unifies fuzzing + property testing:
- Differential testing across implementations (same workload, same expected output)
- Invariant checking under random workloads

## 7. Unsafe Policy

See `docs/UNSAFE.md` for full policy. Summary:

- All unsafe in `raw` submodules only
- Crate roots: `#![deny(unsafe_code)]`; only `raw` modules: `#[allow(unsafe_code)]` on the `mod raw` item. The inner attribute `#![allow(unsafe_code)]` in `raw/mod.rs` propagates to all items within that module.
- Every `unsafe` block: `// SAFETY:` comment (invariant + caller guarantee + failure mode)
- 6-tier verification: unit tests, miri, careful, kani, differential tests, mutants

### Allowed Patterns
- `core::sync::atomic` with documented ordering
- `MaybeUninit` for slot storage
- `core::arch::asm!` for platform fast paths
- `#[repr(C)]` / `#[repr(align)]` casts
- `UnsafeCell` for single-writer interior mutability

### Forbidden
- Raw pointer arithmetic when slice indexing works
- `transmute`
- `unsafe impl Send/Sync` without kani proof
- Unsafe in tests

## 8. Tool Installation

All third-party tools are installed via a `justfile` at the repo root:

```just
# Install all development tools
setup:
    cargo install cargo-deny --locked
    cargo install cargo-careful --locked
    cargo install cargo-mutants --locked
    cargo install cargo-llvm-cov --locked
    cargo install cargo-fuzz --locked
    cargo install --locked kani-verifier && cargo kani setup
    cargo install cargo-criterion --locked
    cargo install prek --locked

# Verify all tools are available
check-tools:
    cargo deny --version
    cargo careful --version
    cargo mutants --version
    cargo llvm-cov --version
    cargo kani --version
```

CI workflows install tools via `cargo install` with caching. Each workflow caches `~/.cargo/bin/` keyed on tool versions.

## 9. Repository Settings

### dependabot.yml
- Cargo ecosystem: weekly schedule, 7-day cooldown
- GitHub Actions: weekly schedule, grouped updates
- Pin all actions to SHA hashes with version comments

### Branch Protection (main)
- Require CI pass before merge
- No direct push to main
- Require at least 1 review for PRs
- Dismiss stale reviews on new pushes

These are documented for manual setup — not auto-configured by this bootstrap.

## 10. Config Files

### deny.toml
- Advisories: deny vulnerabilities, warn unmaintained
- Licenses: allow MIT/Apache-2.0/BSD/ISC, deny copyleft
- Bans: warn multiple versions, deny wildcards
- Sources: deny unknown registries/git

### rust-toolchain.toml
- Channel: stable
- Components: rustfmt, clippy, rust-src
- Targets: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, aarch64-apple-darwin
- Nightly invoked explicitly (`cargo +nightly`) for miri/fuzz/careful

### clippy.toml
- Cognitive complexity threshold: 8
- Too many arguments: 5
- Type complexity: 250

### Workspace Cargo.toml Lints
- clippy::pedantic (warn)
- Panic prevention: deny unwrap_used, panic, unimplemented
- Code hygiene: deny dbg_macro, todo, print_stdout/stderr
- Safety: deny await_holding_lock, large_futures, exit, mem_forget
- Rust: deny unsafe_code (overridden in raw modules), warn missing_docs

### Pre-commit (prek)
- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features --lib -- --quiet`

## 11. CLAUDE.md

Contains:
- All build/test/lint/bench/fuzz/verify commands
- Architecture pointers to philosophy docs and Constantine reference
- Workspace layout with crate descriptions
- no_std rules
- Unsafe policy summary
- Code style (Constantine-inspired coding model)
- Modular strategy pattern explanation
- Benchmark protocol
- Commit and PR rules
- Priority order: correctness > safety > performance > code size > ergonomics

## 12. Test Coverage

| Type | When | Tool | Output |
|---|---|---|---|
| Diff coverage | Every PR | cargo llvm-cov | PR comment |
| Branch coverage | Every PR | cargo llvm-cov --branch | PR comment |
| Full coverage report | Nightly | cargo llvm-cov --html | CI artifact |
| Fuzz corpus coverage | Nightly | cargo llvm-cov on fuzz | CI artifact |

No hard coverage % gate. Diff coverage enforced: new/changed lines must have coverage.

## 13. Differential Testing

A dedicated test module (in `mantis-queue` integration tests) that:
- Runs the same workload against every strategy preset (Portable, Padded, Batched, etc.)
- Asserts identical output sequences for identical input sequences
- Runs on every PR as part of the standard test suite (not nightly-only)

This is a first-class deliverable per the modular strategy doc (section 20, 26.3). The pattern:

```rust
#[test]
fn differential_spsc_presets() {
    let workload = generate_workload(seed: 42, ops: 10_000);
    let result_portable = run_workload::<SpscRingPortable<u64, 1024>>(&workload);
    let result_padded = run_workload::<SpscRingPadded<u64, 1024>>(&workload);
    assert_eq!(result_portable, result_padded);
}
```

Bolero extends this with randomized workloads in nightly.

## 14. What Success Looks Like

After this bootstrap:
- `cargo build`, `cargo test`, `cargo bench` all work
- CI catches fmt/lint/test/safety/perf issues on every PR
- Miri validates unsafe on every PR
- Any agent can read CLAUDE.md and contribute correctly
- First SPSC ring implementation immediately gets the full gauntlet
- Benchmark infrastructure ready for cross-implementation and cross-hardware comparison
- Formal verification harness ready for unsafe proofs
