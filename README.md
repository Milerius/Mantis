# Mantis

[![CI](https://github.com/mantis-sdk/mantis/actions/workflows/ci.yml/badge.svg)](https://github.com/mantis-sdk/mantis/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/mantis-sdk/mantis/graph/badge.svg)](https://codecov.io/gh/mantis-sdk/mantis)

A modular, `no_std`-first Rust foundation for low-latency financial systems across centralized and decentralized markets, with first-class replay, verification, and performance tooling.

## Architecture

```
                        ┌─────────────────────────────┐
                        │         Application          │
                        └──────────┬──────────────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              │                    │                     │
     ┌────────▼───────┐  ┌────────▼────────┐  ┌────────▼────────┐
     │  mantis-queue   │  │  (future crates) │  │  mantis-layout  │
     │  SPSC ring buf  │  │  order book, AMM │  │  struct layout   │
     │  lock-free I/O  │  │  event model ... │  │  cache inspector │
     └───────┬─────────┘  └────────┬────────┘  └─────────────────┘
             │                     │
     ┌───────▼─────────────────────▼────────┐
     │            mantis-core               │
     │   IndexStrategy · PushPolicy         │
     │   Instrumentation · CountingInstr    │
     └───────────────┬──────────────────────┘
                     │
             ┌───────▼─────────┐
             │  mantis-types   │
             │  SeqNum · Slot  │
             │  PushError etc  │
             └─────────────────┘

     ── Tooling (std-only, not depended on by core crates) ──

     ┌─────────────────┐  ┌─────────────────┐
     │  mantis-bench   │  │  mantis-verify  │
     │  criterion +    │  │  kani proofs    │
     │  cycle counters │  │  bolero props   │
     │  JSON reports   │  │  diff testing   │
     └─────────────────┘  └─────────────────┘
```

## Crates

| Crate | Purpose | `no_std` |
|---|---|---|
| [`mantis-core`](crates/core/) | Strategy traits (`IndexStrategy`, `PushPolicy`, `Instrumentation`) | yes |
| [`mantis-types`](crates/types/) | Newtypes and error types (`SeqNum`, `PushError`, `QueueError`) | yes |
| [`mantis-queue`](crates/queue/) | Lock-free SPSC ring buffer with modular strategies | yes |
| [`mantis-platform`](crates/platform/) | Platform abstractions: CT types, cycle counters, ISA primitives | yes |
| [`mantis-bench`](crates/bench/) | Criterion benchmarks + platform cycle counters + JSON reports | no |
| [`mantis-layout`](crates/layout/) | Struct layout and cache-line inspector | no |
| [`mantis-verify`](crates/verify/) | Kani proofs, Bolero property tests, differential testing | no |

## Quick Start

```bash
cargo build --all-features
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo bench
cargo run -p mantis-layout
```

## Benchmarks

Run the full suite including external contenders:

```bash
cargo bench --features bench-contenders
```

Reports are written to `target/bench-report-mantis.json` and `target/bench-report-contenders.json`.

### Latest Results (Apple M4 Pro)

| Workload | Mantis | rtrb | crossbeam |
|---|---:|---:|---:|
| single push+pop (u64) | 2.14 ns | 2.48 ns | 3.93 ns |
| burst 100 (u64) | 422 ns | 332 ns | 546 ns |

## Design Principles

1. **Correctness** — Kani proofs, Miri, differential tests
2. **Safety** — all unsafe isolated in `raw` modules with `// SAFETY:` comments
3. **Performance** — benchmarked, layout-inspected, ASM-verified
4. **`no_std` first** — no heap in hot paths, `alloc` optional

See [CLAUDE.md](CLAUDE.md) for the full development guide and [docs/PROGRESS.md](docs/PROGRESS.md) for current status.

## License

Apache-2.0
