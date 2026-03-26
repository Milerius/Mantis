# Mantis — Low-Latency Financial SDK

## Quick Reference

```
Build:          cargo build --features alloc,std
Build nightly:  cargo +nightly build --all-features
Test:           cargo test --features alloc,std
Test nightly:   cargo +nightly test --all-features
Test no_std:    cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features
Lint:           cargo clippy --all-targets --features alloc,std -- -D warnings
Lint nightly:   cargo +nightly clippy --all-targets --all-features -- -D warnings
Format:         cargo fmt --all
Format check:   cargo fmt --all --check
Deny:           cargo deny check
Miri:           cargo +nightly miri test -p mantis-queue
Careful:        cargo +nightly careful test
Bench:          cargo bench --bench spsc
Bench native:   RUSTFLAGS='-C target-cpu=native' cargo bench --bench spsc
Bench + ext:    cargo bench --bench spsc --features bench-contenders
Fuzz:           cargo +nightly fuzz run <target>
Layout:         cargo run -p mantis-layout
Kani:           cargo kani -p mantis-verify
Coverage:       cargo llvm-cov --all-features
```

## Architecture

See `philosophy/fin_sdk_oss_blueprint.md` for full SDK vision.
See `philosophy/benchmark_tooling_modular_strategy_design.md` for container strategy.
See `.claude/memory/constantine-reference.md` for reference architecture patterns.
See `docs/PROGRESS.md` for current project status and phase tracking.

## Progress Tracking

**Update `docs/PROGRESS.md` after completing meaningful work:**
- Check off completed items
- Update the crate status table (test counts, benchmark status, verification status)
- Move phase status from "Not Started" to "In Progress" / "Complete" with dates
- Add new sub-items if scope is discovered during implementation

## Workspace Layout

```
crates/core/     mantis-core      Traits, strategy definitions           (no_std)
crates/types/    mantis-types     IDs, newtypes, error types             (no_std)
crates/queue/    mantis-queue     SPSC ring + queue primitives           (no_std)
crates/platform/ mantis-platform  Platform abstractions, CT types, SIMD, counters  (no_std)
crates/bench/    mantis-bench     Criterion + custom perf harness        (std)
crates/layout/   mantis-layout    Struct layout / cache-line inspector   (std)
crates/verify/   mantis-verify    Kani proofs, bolero property tests     (std)
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
- `#[inline]` only on measured hot functions, never speculatively

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

- Imperative mood, <=72 char subject, one logical change per commit
- Run fmt + clippy + test before committing
- Feature branches, never push directly to main
