# SPSC Queue Latency Benchmark

Cross-language, two-thread latency benchmark for single-producer single-consumer queues.

## Why This Exists

Previous SPSC benchmarks in this project measured push+pop on a **single thread**, which kept everything in L1 cache and produced ~2.5ns numbers that were meaningless for real inter-thread communication. This benchmark replaces that with a proper two-thread setup where producer and consumer run on separate cores with cache-coherence traffic on every operation.

Background: <https://hftuniversity.com/post/the-mantis-spsc-queue-a-case-study-in-how-not-to-benchmark>

## Methodology

- **Two threads**, each pinned to a dedicated core via `core_affinity` (Rust) or `pthread_setaffinity_np` (C++).
- Cores should be on the **same CCD** and isolated with `isolcpus` to eliminate scheduler noise.
- Timestamps use **`lfence; rdtsc`** (serialized rdtsc) on both producer and consumer sides.
- Message type: **`Message48`** -- a 48-byte `#[repr(C, align(16))]` struct representing a realistic financial message (price, quantity, order_id, sequence, etc.).
- Queue capacity: **1024 slots**.
- **10,000 warmup** messages (discarded), then **1,000,000 measured** messages per run.
- **5 runs** per queue implementation.
- Latency is measured as `consumer_rdtsc - producer_rdtsc` in cycles for each message.
- Results are real histograms with percentile breakdowns, not averages.
- No FFI boundary in the measurement path -- Rust queues are benchmarked from Rust, C++ queues from C++.

## Queues Tested

| Queue | Language | Notes |
|---|---|---|
| Mantis `SpscRing` (inline) | Rust | Inline publish strategy, no copy on push |
| Mantis `SpscRing` (copy) | Rust | Copy publish strategy |
| rtrb | Rust | Popular Rust SPSC crate |
| rigtorp SPSCQueue | C++ | Erik Rigtorp's well-known lock-free queue |
| Drogalis SPSCQueue | C++ | Max Drogalis' SPSC implementation |

## Quick Start

### Prerequisites

- Linux x86_64 with `isolcpus` configured (e.g., `isolcpus=2,3` in kernel cmdline)
- Rust nightly toolchain
- CMake + C++17 compiler (for C++ contenders)

### Local Run

```bash
# Build and run all Rust queues on cores 2 and 3
cd benchmarks/rust
cargo build --release
./target/release/mantis-spsc-bench --producer-core 2 --consumer-core 3

# Build and run C++ queues
cd benchmarks/cpp
mkdir -p build && cd build
cmake -DCMAKE_BUILD_TYPE=Release ..
make
./spsc_bench --producer-core 2 --consumer-core 3
```

### Scripted Run

```bash
# Validate system configuration (isolcpus, governor, turbo)
scripts/prepare_system.sh

# Run the full benchmark suite
scripts/run_bench.sh --producer-core 2 --consumer-core 3

# Compare results across all queues
python3 scripts/compare.py results/
```

### Remote Execution

```bash
# Deploy to a remote machine and run
scripts/deploy_and_run.sh user@host --producer-core 2 --consumer-core 3
```

## Interpreting Results

Results are reported in **CPU cycles**, not nanoseconds. Cycles are architecture-independent and avoid the need to know the exact clock frequency. To convert: `ns = cycles / (GHz)`.

| Metric | Meaning |
|---|---|
| **p50** | Median latency -- the typical case |
| **p99** | 99th percentile -- worst 1 in 100 |
| **p999** | 99.9th percentile -- worst 1 in 1,000 |
| **p9999** | 99.99th percentile -- worst 1 in 10,000 |
| **max** | Single worst observation across all measured messages |
| **mean** | Arithmetic mean -- useful for throughput estimation |

A good SPSC queue on isolated cores of the same CCD should show p50 in the 20-40 cycle range with tight p99. Large gaps between p50 and p99 indicate contention, cache-line bouncing, or scheduler interference.

## Perf Analysis

Profile the benchmark under `perf` to inspect cache behavior and generate flamegraphs:

```bash
# CPU profiling (cycles, instructions, IPC)
scripts/perf_profile.sh ./rust/target/release/mantis-spsc-bench \
    --producer-core 2 --consumer-core 3 --queue mantis-inline --runs 1

# Cache miss analysis (L1d, LLC loads/misses)
scripts/perf_cache.sh ./rust/target/release/mantis-spsc-bench \
    --producer-core 2 --consumer-core 3 --queue mantis-inline --runs 1

# Generate a flamegraph SVG
scripts/perf_flamegraph.sh ./rust/target/release/mantis-spsc-bench \
    --producer-core 2 --consumer-core 3 --queue mantis-inline --runs 1
```

## Adding a Contender

### Rust

1. Create `benchmarks/rust/src/queues/your_queue.rs`.
2. Implement the `QueueBench`, `QueueProducer`, and `QueueConsumer` traits for your queue type.
3. Add `pub mod your_queue;` to `benchmarks/rust/src/queues/mod.rs`.
4. Add the match arm in `run_queue()` in `benchmarks/rust/src/main.rs`.
5. Add the crate to `benchmarks/rust/Cargo.toml` as a dependency.

### C++

1. Create a header in `benchmarks/cpp/src/queues/your_queue.hpp` wrapping the third-party queue.
2. Expose `try_push(const Message48&)` and `try_pop(Message48&)` methods matching the existing interface.
3. Add the queue to the dispatch logic in `benchmarks/cpp/src/main.cpp`.
4. If vendoring the source, place it under `benchmarks/cpp/vendor/`.

## Known Limitations

- **Same-CCD only.** Cross-CCD measurements add interconnect latency that dwarfs queue overhead; results would measure topology, not the queue.
- **Linux x86_64 only.** The benchmark relies on `rdtsc`, `core_affinity`/`pthread_setaffinity_np`, and `isolcpus`. macOS and ARM are not supported.
- **Fixed capacity.** Queue capacity is hardcoded to 1024 slots. Varying capacity is a future option but not yet parameterized.
- **Consumer copies on pop.** The consumer calls `try_pop(&mut msg)` which copies the message out of the ring buffer. This is intentional -- it matches real consumer patterns -- but means the benchmark includes one 48-byte copy in the measured path.
