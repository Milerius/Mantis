# SPSC Queue Benchmark Results

## Summary

Mantis `SpscRingFast` (BranchWrap) achieves **472 cycles/op** mean latency on a two-thread cross-core benchmark, matching or beating the best C++ SPSC queue implementations.

| Queue | Language | Best | Median | Stability |
|-------|----------|------|--------|-----------|
| **mantis SpscRingFast (BranchWrap)** | Rust | **472** | **478** | **+/-8** |
| mantis SpscRing (Pow2Masked) | Rust | 554 | 565 | +/-14 |
| mantis SpscRingCopy | Rust | 552 | 562 | +/-11 |
| C++ rigtorp::SPSCQueue | C++ | 510 | 1315 | +/-400 |
| rtrb (Rust, Result API) | Rust | 1090 | 2345 | +/-750 |

All values in TSC cycles (3.65 GHz invariant TSC). Lower is better.

## Hardware

| Component | Value |
|-----------|-------|
| CPU | AMD Ryzen 7 PRO 8700GE (Zen 4, 8C/16T) |
| Base clock | 3.6 GHz |
| Boost clock | 5.18 GHz (during benchmark) |
| TSC frequency | 3.65 GHz (invariant, `constant_tsc nonstop_tsc`) |
| RAM | 64 GB DDR5 ECC |
| L1d cache | 32 KB per core |
| L2 cache | 1 MB per core |
| L3 cache | 16 MB shared (single CCD) |
| Cache line | 64 bytes |
| Storage | 2x 512 GB Samsung NVMe (RAID-1) |
| Provider | Hetzner AX42-U (HEL1-DC7, Helsinki) |

## OS and Toolchain

| Component | Value |
|-----------|-------|
| OS | Ubuntu 24.04.3 LTS |
| Kernel | 6.8.0-107-generic |
| Rust | rustc 1.96.0-nightly (bcded3316 2026-04-06) |
| C++ | g++ 13.3.0 |
| RUSTFLAGS | `-C target-cpu=native` |
| CXXFLAGS | `-O2 -march=native -std=c++20` |
| LTO | Enabled (Rust: `lto = true, codegen-units = 1`) |

## Isolation Configuration

| Setting | Value |
|---------|-------|
| Kernel params | `isolcpus=2,3,10,11 nohz_full=2,3,10,11 rcu_nocbs=2,3,10,11 amd_pstate=passive processor.max_cstate=1 idle=poll` |
| Producer core | CPU 2 (physical core 2, socket 0) |
| Consumer core | CPU 3 (physical core 3, socket 0) |
| Topology | Same CCD, same L3, different physical cores, not SMT siblings |
| CPU governor | `performance` |
| Turbo boost | Enabled (5.18 GHz observed) |
| NMI watchdog | Disabled (`kernel.watchdog=0 kernel.nmi_watchdog=0`) |
| IRQ affinity | All IRQs steered to non-isolated cores (`0,1,4-9,12-15`) |
| irqbalance | Stopped and disabled |
| THP | Disabled (`echo never > /sys/kernel/mm/transparent_hugepage/enabled`) |
| Scheduling | `chrt -f 99` (FIFO real-time priority) |
| CPU mask | `taskset -c 2,3` (enforced on entire process) |
| perf_event_paranoid | 1 |

## Benchmark Protocol

Matches the [HFT University Ring Buffer Challenge](https://hftuniversity.com/) protocol:

1. **Two threads**: producer and consumer on separate pinned physical cores
2. **Message**: 48-byte `#[repr(C, align(16))]` struct (timestamp, symbol_id, side, price, quantity, order_id, sequence)
3. **Queue capacity**: 1024 (power of two)
4. **Timestamping**: `lfence; rdtsc` inline assembly (serialized TSC read)
5. **Producer**: stamps `rdtsc` into `msg.timestamp`, then `push(msg)` in spin loop
6. **Consumer**: `pop(&mut msg)` in spin loop, then `rdtsc`, accumulates `sum += now - msg.timestamp`
7. **Metric**: `total_sum / total_ops` = mean cycles per operation
8. **Operations**: 1,000,000 messages per run
9. **Warmup**: 1 full run discarded before measurement
10. **Runs**: 10-15 consecutive runs, report best and median

## Queue Variants Tested

### Rust (mantis-queue library)

| Variant | Index Strategy | API | Notes |
|---------|---------------|-----|-------|
| `SpscRingFast<T, N>` | `BranchWrap` | `push_shared/pop_shared (&self)` | Fastest. Branch predictor optimization. |
| `SpscRing<T, N>` | `Pow2Masked` | `push_shared/pop_shared (&self)` | Default. Bitwise AND wrapping. |
| `SpscRingCopy<T, N>` | `Pow2Masked` | `push_shared/pop_shared (&self)` | Copy-optimized (`&T`/`&mut T`). |

All variants use:
- `CacheLine`-colocated producer/consumer fields (64B on x86_64)
- `#[inline(always)]` on hot-path functions
- `bool` return (no `Result` overhead)
- `&self` shared reference (no `&mut` aliasing/`noalias` interference)

### Rust (external)

| Queue | API | Notes |
|-------|-----|-------|
| rtrb 0.3 | `push(T) -> Result`, `pop() -> Result<T>` | Result return-by-value adds ~500+ cycles overhead |

### C++ (native, no FFI)

| Queue | API | Notes |
|-------|-----|-------|
| rigtorp::SPSCQueue | `try_push(T) -> bool`, `front() -> T*`, `pop()` | Gold standard C++ SPSC. Zero-copy read via `front()`. |

## Key Findings

### 1. Cache-line colocation is the biggest optimization

Colocating `head` + `tail_cached` on the same 64-byte cache line (producer-local) and `tail` + `head_cached` on another (consumer-local) saved ~400 cycles/op. Previously each field was on a separate 128-byte padded region — 4 cache lines touched per operation instead of 2.

### 2. BranchWrap beats Pow2Masked by ~85 cycles

`if index >= capacity { 0 } else { index }` vs `index & (capacity - 1)`. The branch predictor learns the wrap-around almost never happens (once per 1024 ops), making the branch effectively free. The bitwise AND always executes.

### 3. Result<T> return costs ~500+ cycles

rtrb's `pop() -> Result<T, PopError>` forces a 48-byte `T` return through the `Result` discriminant on every successful pop. The `bool` + raw pointer API (`pop_shared(&self, out: *mut T) -> bool`) eliminates this entirely.

### 4. `&self` vs `&mut self` matters for LLVM

Using `&mut self` from two threads (even with SPSC safety) is UB due to Rust's `noalias` guarantee on `&mut`. LLVM may generate extra reloads or prevent optimizations. The `push_shared`/`pop_shared` API uses `&self`, which is sound under the SPSC protocol (engine fields accessed via atomics and cells).

### 5. Rust stability vs C++ variance

Rust mantis consistently hits the same latency (478 +/- 8 cycles) across all runs. C++ rigtorp shows bimodal behavior (510 or 1300+), likely due to initial thread scheduling jitter determining whether the consumer stays ahead of or behind the producer.

### 6. `#[inline(always)]` + LTO are essential

Without `#[inline(always)]` on `Storage::capacity()`, `Storage::slot_ptr()`, and the push/pop hot paths, the compiler may not constant-fold the capacity `N` or inline the slot pointer arithmetic across crate boundaries. LTO (`lto = true, codegen-units = 1`) ensures cross-crate inlining.

## How to Reproduce

```bash
# On the benchmark server (Ubuntu 24.04, isolcpus configured)
cd benchmarks/rust
RUSTFLAGS='-C target-cpu=native' cargo +nightly build --release

# Run all variants
chrt -f 99 taskset -c 2,3 ./target/release/mantis-spsc-bench \
    --queue raw --producer-core 2 --consumer-core 3 \
    --messages 1000000 --runs 10

# C++ comparison
cd ../cpp
cmake -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_FLAGS_RELEASE="-O2 -march=native -flto -DNDEBUG"
cmake --build build --parallel
chrt -f 99 taskset -c 2,3 ./build/spsc-bench \
    --queue all --producer-core 2 --consumer-core 3 \
    --messages 1000000 --warmup 50000 --runs 10 \
    --output-dir results
```

## Previous Methodology Flaws (Addressed)

This benchmark suite was created in response to an [HFT University article](https://hftuniversity.com/post/the-mantis-spsc-queue-a-case-study-in-how-not-to-benchmark) that identified 7 methodology flaws in the original benchmarks:

1. Single-threaded push+pop (measured L1 cache, not cross-core latency) -- **Fixed: two threads on isolated cores**
2. FFI-handicapped C++ contenders -- **Fixed: separate native C++ binary**
3. Forced copy on rigtorp's zero-copy pop -- **Fixed: native C++ API**
4. MPMC (crossbeam) compared against SPSC -- **Fixed: removed**
5. Batch path compared against nothing -- **Fixed: removed from comparison**
6. Gaussian percentile approximation -- **Fixed: real histogram or sum/count**
7. SIMD policy not default -- **Documented: DefaultCopyPolicy is memcpy, SIMD requires nightly feature**
