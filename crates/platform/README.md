# mantis-platform

Platform abstractions for the Mantis SDK, achieving full parity with
[Constantine's](https://github.com/mratsim/constantine) `platforms/` module.

Provides constant-time types, extended-precision intrinsics, cycle counters,
and ISA-specific primitives — all `no_std` compatible.

## Module Layout

```
constant_time/   Ct<T>, CTBool<T>, constant-time arithmetic, multiplexers,
                 constant-time division
intrinsics/      Extended-precision arithmetic (wide mul, add-with-carry,
                 sub-with-borrow), prefetch hints, copy policies
bithacks         Bit manipulation utilities (clz, ctz, popcount, byte-swap)
config           Compile-time platform configuration (word size, endianness,
                 feature detection flags)
metering/        CycleCounter trait, Measurement wrapper,
                 platform-specific counter implementations
isa_x86/         x86_64 assembler types, CPUID detection, RDTSC counter
isa_arm64/       ARM64 assembler types, condition codes,
                 Kperf/PMU cycle counters
pad              CachePadded<T> — 128-byte alignment to prevent false sharing
cpudetect        CPU model name detection (used in benchmark reports)
```

## Feature Flags

| Flag | Default | Effect |
|---|---|---|
| `std` | off | Enable `std`-dependent paths (CPU name detection, OS counters) |
| `asm` | off | Enable inline assembly paths (RDTSC, ARM PMU, wide-mul asm) |
| `nightly` | off | Enable nightly-only intrinsics and unstable features |

The crate is `no_std` by default. Enable `std` for benchmark tooling and OS
integration. Enable `asm` for production-grade cycle counters and hot-path
assembly.

## Usage

```rust
use mantis_platform::pad::CachePadded;
use mantis_platform::metering::{CycleCounter, Measurement};

// Cache-line isolate a hot value
let head = CachePadded::new(0u64);

// Read a cycle counter
let start = Measurement::now();
// ... work ...
let elapsed = start.elapsed();
```

For constant-time operations:

```rust
use mantis_platform::constant_time::{Ct, CTBool};

let a = Ct::new(42u64);
let b = Ct::new(42u64);
let equal: CTBool<u64> = a.ct_eq(b);
```

## Design Notes

- No runtime dispatch in hot paths — platform selection is `cfg(target_arch)`
- Assembly paths live in `isa_x86/assembly/` and `isa_arm64/assembly/`
- All platform variants are differential-tested against the portable baseline
- Unsafe code is isolated in `raw` submodules with `// SAFETY:` comments
