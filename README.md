<p align="center">
  <img src="assets/banner.png" alt="Mantis — Financial Low-Latency HFT Library in Rust" width="600">
</p>

<h1 align="center">
  <img src="assets/logo.png" alt="Mantis logo" width="40" valign="middle">
  Mantis
</h1>

<p align="center">
  A modular, <code>no_std</code>-first Rust foundation for low-latency financial systems<br>
  across centralized and decentralized markets, with first-class replay, verification, and performance tooling.
</p>

<p align="center">
  <a href="https://github.com/Milerius/Mantis/actions/workflows/ci.yml"><img src="https://github.com/Milerius/Mantis/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://codecov.io/gh/Milerius/Mantis"><img src="https://codecov.io/gh/Milerius/Mantis/graph/badge.svg" alt="codecov"></a>
</p>

## Architecture

```
                        ┌─────────────────────────────┐
                        │         Application          │
                        │  (bot, engine, venue adapter) │
                        └──────────┬──────────────────┘
                                   │  wires SpscRingCopy<HotEvent, N>
              ┌────────────────────┼────────────────────┐
              │                    │                     │
     ┌────────▼───────┐  ┌────────▼────────┐  ┌────────▼────────┐
     │  mantis-queue   │  │  mantis-events  │  │  mantis-layout  │
     │  SPSC ring buf  │  │  HotEvent 64B   │  │  struct layout   │
     │  lock-free I/O  │  │  event language │  │  cache inspector │
     └───────┬─────────┘  └────────┬────────┘  └─────────────────┘
             │                     │
             │          ┌──────────▼──────────┐
             │          │    mantis-types      │
             ├─────────►│  Ticks · Lots · Side │
             │          │  Timestamp · OrderId │
             │          │  InstrumentId · etc  │
             │          └──────────┬───────────┘
             │                     │
     ┌───────▼─────────┐  ┌───────▼──────────┐
     │   mantis-core   │  │   mantis-fixed   │
     │  IndexStrategy   │  │  FixedI64<D>     │
     │  PushPolicy      │  │  decimal engine  │
     │  Instrumentation │  │  (boundary only) │
     └───────┬─────────┘  └──────────────────┘
             │
     ┌───────▼──────────┐
     │ mantis-platform  │
     │  CachePadded     │
     │  cycle counters  │
     │  CT types, SIMD  │
     └──────────────────┘

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
| [`mantis-platform`](crates/platform/) | Platform abstractions: cache padding, CT types, cycle counters, SIMD | yes |
| [`mantis-fixed`](crates/fixed/) | `FixedI64<D>` compile-time fixed-point decimal engine | yes |
| [`mantis-types`](crates/types/) | Domain types: `Ticks`, `Lots`, `Side`, `Timestamp`, `InstrumentMeta` | yes |
| [`mantis-events`](crates/events/) | Hot event language: 64B `HotEvent` envelope for SPSC transport | yes |
| [`mantis-queue`](crates/queue/) | Lock-free SPSC ring buffer with modular strategies | yes |
| [`mantis-seqlock`](crates/seqlock/) | Lock-free sequence lock primitive | yes |
| [`mantis-core`](crates/core/) | Strategy traits (`IndexStrategy`, `PushPolicy`, `Instrumentation`) | yes |
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
