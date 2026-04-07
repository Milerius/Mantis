<p align="center">
  <img src="assets/banner.png" alt="Mantis — Financial Low-Latency HFT Library in Rust" width="600">
</p>

<h1 align="center">
  <img src="assets/logo.png" alt="Mantis logo" width="40" valign="middle">
  Mantis
</h1>

<p align="center">
  A modular, <code>no_std</code>-first Rust foundation for low-latency financial systems<br>
  across centralized and decentralized markets.
</p>

<p align="center">
  <a href="https://github.com/Milerius/Mantis/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Milerius/Mantis/ci.yml?style=flat-square&logo=github&label=CI" alt="CI"></a>
  <a href="https://codecov.io/gh/Milerius/Mantis"><img src="https://img.shields.io/codecov/c/github/Milerius/Mantis?style=flat-square&logo=codecov&label=coverage" alt="Coverage"></a>
  <a href="https://github.com/Milerius/Mantis/actions/workflows/nightly.yml"><img src="https://img.shields.io/github/actions/workflow/status/Milerius/Mantis/nightly.yml?style=flat-square&logo=github&label=nightly" alt="Nightly"></a>
  <img src="https://img.shields.io/badge/rust-nightly-93450a?style=flat-square&logo=rust" alt="Rust Nightly">
  <img src="https://img.shields.io/badge/no__std-first-orange?style=flat-square" alt="no_std">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="License"></a>
</p>

<p align="center">
  <a href="https://codecov.io/gh/Milerius/Mantis"><img src="https://img.shields.io/codecov/c/github/Milerius/Mantis?style=for-the-badge&label=coverage" alt="Coverage"></a>
  <a href="https://github.com/Milerius/Mantis/actions/workflows/nightly.yml"><img src="https://img.shields.io/github/actions/workflow/status/Milerius/Mantis/nightly.yml?style=for-the-badge&label=miri%20%2B%20kani" alt="Miri + Kani"></a>
</p>

<p align="center">
  <b>42ns market state engine · 130ns cross-core SPSC · Zero allocation on hot path · Miri + Kani verified</b>
</p>

---

## Why Mantis?

| | Mantis | Typical HFT Software | Top-Tier (FPGA) |
|---|---|---|---|
| **Event processing** | **42ns** p50 | 1-10µs | <100ns |
| **SPSC ring (cross-core)** | **130ns** (472 cycles) | 200-500ns | N/A |
| **Memory allocation** | **Zero** on hot path | Minimal | Fixed in fabric |
| **Verification** | Miri + Kani + Fuzz + Bolero | Unit tests | Hardware proofs |
| **Replay** | **Deterministic** | Best-effort | Hard |
| **`no_std`** | All core crates | Rarely | N/A |

SPSC latency measured on isolated cores (AMD Ryzen 7 PRO 8700GE, `isolcpus`, `rdtsc`).
See [`benchmarks/RESULTS.md`](benchmarks/RESULTS.md) for full methodology and comparison with C++ rigtorp.

---

## Architecture

```
                    ┌─────────────────────────────┐
                    │      Trading Application     │
                    └──────────────┬───────────────┘
                                   │
              ┌────────────────────┼────────────────────┐
              │                    │                     │
     ┌────────▼────────┐ ┌────────▼────────┐  ┌────────▼────────┐
     │ mantis-strategy  │ │  mantis-queue   │  │ mantis-market-  │
     │                  │ │                 │  │ state           │
     │  Strategy trait  │ │  SPSC ring buf  │  │  ArrayBook      │
     │  Position, PnL   │ │  lock-free I/O  │  │  Engine, BBO    │
     │  QueueEstimator  │ │  130ns x-core   │  │  42ns/event     │
     │  OrderTracker    │ └────────┬────────┘  └────────┬────────┘
     └────────┬─────────┘          │                    │
              │           ┌────────▼────────┐  ┌────────▼────────┐
              │           │  mantis-events  │  │  mantis-types   │
              ├──────────►│  HotEvent 64B   │  │  Ticks · Lots   │
              │           │  1 cache line   │  │  SignedLots      │
              │           └────────┬────────┘  │  InstrumentId    │
              │                    │           └────────┬────────┘
              │           ┌────────▼────────┐  ┌────────▼────────┐
              │           │  mantis-fixed   │  │ mantis-platform │
              └──────────►│  FixedI64<D>    │  │  CachePadded    │
                          │  1.10ns mul     │  │  CT types, SIMD │
                          └─────────────────┘  │  cycle counters │
                                               └─────────────────┘
```

<details>
<summary><b>Data Flow — Hot Path</b></summary>

```
Feed Handler → [SPSC Ring] → Market State Engine → Strategy.on_event() → [SPSC Ring] → Execution
                                    ↓
                             Books · BBO · Micro Price
                             Queue Position · Take Rate
                             Fill Probability · Exposure
```

Each strategy is a self-contained state machine. Same code path live and replay.
Feed the same event tape → get identical order intents.

</details>

---

## Highlights

- **42ns market state engine** — single-core event processing, no locks, no allocation on hot path
- **130ns cross-core SPSC** — measured on isolated cores with `rdtsc`, beating C++ rigtorp on same hardware
- **Zero-alloc hot path** — fixed-size arrays, `repr(C)` types, `no_std` everywhere
- **Formally verified** — Miri (zero UB), Kani (bounded model checking), Bolero (property tests), fuzz targets
- **Deterministic replay** — event-driven strategy trait: same tape = same intents
- **14 modular crates** — compose what you need, leave the rest
- **L2 queue position model** — probabilistic fill estimation using `PowerProbQueueFunc`

---

## Crates

| Crate | Purpose | `no_std` | Tests |
|---|---|---|---:|
| [`mantis-strategy`](crates/strategy/) | Strategy trait, Position, OrderTracker, QueueEstimator, RiskLimits | yes | 38 |
| [`mantis-market-state`](crates/market-state/) | Market-state engine: `ArrayBook`, `MarketStateEngine`, TopOfBook | yes | 17 |
| [`mantis-events`](crates/events/) | Hot event language: 64B `HotEvent` envelope for SPSC transport | yes | 62 |
| [`mantis-queue`](crates/queue/) | Lock-free SPSC ring buffer with modular strategies | yes | 31 |
| [`mantis-types`](crates/types/) | Domain types: `Ticks`, `Lots`, `SignedLots`, `Side`, `InstrumentId` | yes | 98 |
| [`mantis-fixed`](crates/fixed/) | `FixedI64<D>` compile-time fixed-point decimal engine | yes | 110 |
| [`mantis-platform`](crates/platform/) | Platform abstractions: cache padding, CT types, cycle counters, SIMD | yes | 164 |
| [`mantis-seqlock`](crates/seqlock/) | Lock-free sequence lock primitive | yes | 1 |
| [`mantis-core`](crates/core/) | Strategy traits (`IndexStrategy`, `PushPolicy`, `Instrumentation`) | yes | 1 |
| [`mantis-registry`](crates/registry/) | Instrument registry with venue bindings | yes | — |
| [`mantis-transport`](crates/transport/) | WebSocket feed handlers (Polymarket, Binance) | no | — |
| [`mantis-bench`](crates/bench/) | Criterion benchmarks + platform cycle counters + JSON reports | no | 11 |
| [`mantis-layout`](crates/layout/) | Struct layout and cache-line inspector | no | 6 |
| [`mantis-verify`](crates/verify/) | Kani proofs, Bolero property tests, differential testing | no | 13 |

---

## Benchmarks

### SPSC Ring — Two-Thread Cross-Core Latency

Measured on AMD Ryzen 7 PRO 8700GE with `isolcpus`, `rdtsc`, `chrt -f 99`.
48-byte messages, capacity 1024, 1M operations per run.

| Queue | Language | Mean (TSC cycles) | Stability |
|-------|----------|---:|---:|
| **Mantis `SpscRingFast`** | Rust | **472** | +/-8 |
| Mantis `SpscRing` | Rust | 554 | +/-14 |
| Mantis `SpscRingCopy` | Rust | 552 | +/-11 |
| rigtorp `SPSCQueue` | C++ | 510 best, 1315 median | +/-400 |
| rtrb | Rust | 1090 best, 2345 median | +/-750 |

At 3.65 GHz TSC, 472 cycles = **129 ns** per cross-core message handoff.

See [`benchmarks/RESULTS.md`](benchmarks/RESULTS.md) for full methodology, hardware configuration, and reproduction steps.

### Fixed-Point Math

| Operation | Mantis | `fixed` crate | `rust_decimal` |
|---|---:|---:|---:|
| mul | **1.10 ns** | 1.20 ns | -- |
| add | **0.28 ns** | -- | 1.12 ns (4x slower) |

Run benchmarks:
```bash
# Fixed-point
cargo +nightly bench --bench fixed

# SPSC (requires isolated cores on Linux)
cd benchmarks/rust
RUSTFLAGS='-C target-cpu=native' cargo +nightly build --release
chrt -f 99 taskset -c 2,3 ./target/release/mantis-spsc-bench \
    --producer-core 2 --consumer-core 3 --messages 1000000 --runs 10
```

---

## Quick Start

```bash
# Build everything
cargo +nightly build --features alloc,std

# Run tests
cargo +nightly test --features alloc,std

# Check coverage
cargo +nightly llvm-cov --all-features

# Inspect struct layouts
cargo run -p mantis-layout

# Run fixed-point benchmarks
cargo +nightly bench --bench fixed
```

---

<details>
<summary><h2>🧪 Verification Strategy</h2></summary>

| Tool | What It Catches | Scope |
|---|---|---|
| **Miri** | Undefined behavior, data races, use-after-free | All `no_std` crates on every PR |
| **Kani** | Arithmetic overflow, out-of-bounds, invariant violations | 4 bounded model checking proofs |
| **Bolero** | Edge cases via property-based + fuzz testing | 10+ property tests |
| **Fuzz** | Crash bugs in parsing + serialization | 2 fuzz targets (fixed-point) |
| **Differential** | Portable vs platform-specific divergence | 3 cross-variant comparisons |
| **Careful** | Additional UB detection beyond Miri | Full workspace |

Every PR runs: fmt → clippy → test → no_std test → Miri → coverage → deny → doc build.

Nightly runs add: mutant testing, extended Miri, full coverage, ASM inspection, Kani proofs, fuzz.

</details>

<details>
<summary><h2>🎯 Strategy Design</h2></summary>

```rust
use mantis_strategy::{Strategy, OrderIntent, MAX_INTENTS_PER_TICK};
use mantis_events::HotEvent;

struct MyStrategy { /* own state, own engine, own position */ }

impl Strategy for MyStrategy {
    const STRATEGY_ID: u8 = 0;
    const NAME: &'static str = "my-strategy";

    fn on_event(
        &mut self,
        event: &HotEvent,
        intents: &mut [OrderIntent; MAX_INTENTS_PER_TICK],
    ) -> usize {
        // Process event, update internal state, emit order intents
        0
    }
}
```

**Key properties:**
- No generics on the trait — implementation details stay inside the strategy
- Associated consts (`STRATEGY_ID`, `NAME`) — zero runtime overhead
- Enum dispatch in bot binary — no vtable, no heap, fully inlined
- Each strategy owns its own `MarketStateEngine` — no shared mutable state
- Replay: feed the same event tape → get identical intents

</details>

<details>
<summary><h2>📊 Live POC — Polymarket + Binance</h2></summary>

A working Nim prototype (`polymarket-bot-poc/`) captures Polymarket prediction markets + Binance reference feeds with an FTXUI terminal dashboard.

**Features:**
- Multi-market: BTC, SOL, ETH up-or-down 5m markets
- 6-thread architecture: ingest(2) + engine + telemetry + dashboard + main
- FTXUI 3-column trading terminal with Canvas charts
- Depth ladders (UP/DOWN PM books + Binance depth20 L2)
- Probability history, latency histogram, event rate sparkline
- Queue gauges, feed status, trade tape, market tab switching
- Binary mmap tape output with zstd compression
- Engine latency: p50=42ns, p99=1-5µs
- CPU: ~9% with yield-on-idle

See [`polymarket-bot-poc/README.md`](polymarket-bot-poc/README.md) for build instructions.

</details>

<details>
<summary><h2>🏛️ Design Principles</h2></summary>

1. **Correctness first** — Kani proofs, Miri, differential tests
2. **Safety** — all unsafe isolated in `raw` modules with `// SAFETY:` comments
3. **Performance** — benchmarked, layout-inspected, ASM-verified
4. **`no_std` first** — no heap in hot paths, `alloc` optional
5. **Replay-friendly** — every component is a deterministic state machine
6. **Venue-agnostic** — no prediction market concepts in core SDK

See [CLAUDE.md](CLAUDE.md) for the full development guide.

</details>

<details>
<summary><h2>📋 Project Status</h2></summary>

See [docs/PROGRESS.md](docs/PROGRESS.md) for detailed tracking.

**Phase 1 — Completed:**
- SPSC ring buffer with strategy pattern
- Benchmark harness (RDTSC, kperf, criterion)
- Platform abstractions (CT types, SIMD, cache padding)
- Fixed-point decimal engine
- 64B HotEvent model (8 variants)
- Sequence lock
- Market state engine (ArrayBook, BBO tracking)
- Strategy runtime (trait, position, queue estimator, risk)

**In Progress (by collaborators):**
- Instrument registry with venue bindings
- WebSocket transport (Polymarket, Binance)

**Next:**
- Execution engine (order management, signing, fill routing)
- Risk management (per-strategy + global wallet)
- Capture/replay framework
- `mantis-prediction` crate (binary outcome positions)

</details>

---

## License

Apache-2.0
