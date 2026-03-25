# Constantine Reference Patterns

Always reference Constantine (https://github.com/mratsim/constantine) as the mental model for:

## Platform-Specific Code
- Compile-time CPU detection via cfg flags, NOT runtime dispatch in hot paths
- Inline asm with structured operand modeling (Mantis equivalent: `core::arch::asm!`)
- ASM toggle flag: test with ASM enabled AND disabled for fallback validation
- Platform-specific intrinsics isolated in dedicated modules (`platforms/intrinsics/`)

## Benchmark Architecture
- x86-64: RDTSC with lfence barrier for cycle counting
- ARM64: monotonic time fallback (no reliable cycle counter)
- Report includes: CPU name, compiler version, ops/sec, ns/op, estimated cycles
- `{.noinline.}` + volatile tricks to prevent optimizer interference
- Warmup phase to stabilize CPU frequency

## CI Matrix Pattern
| OS | CPU | ASM | Purpose |
|---|---|---|---|
| Linux | x86_64 | yes/no | Primary + fallback |
| Linux | ARM64 | yes/no | ARM hardware |
| macOS | ARM64 | yes/no | Apple Silicon |
| Windows | x86_64 | yes/no | Cross-platform |

## Code Safety
- Zero external deps in crypto/hot-path core
- Effect tracking: allocation policy, side-channel resistance at compile time
- Constant-time properties enforced via type system / tags
- Differential testing against reference implementations

## Where Mantis Goes Further
- Fuzz targets (Constantine lacks them)
- JSON/CSV benchmark export + cross-run comparison matrix
- Miri in CI for unsafe validation
- cargo-mutants for test quality
- Structured UNSAFE.md policy
- Modular strategy pattern (multiple implementations per primitive)
