# Contributing to Mantis

Thanks for your interest in contributing to Mantis! This guide covers what you need to get started.

## Prerequisites

Mantis requires the **nightly** Rust toolchain. The repo pins the channel via `rust-toolchain.toml`, so `rustup` handles it automatically.

## Build & Test

```bash
# Build
cargo +nightly build --features alloc,std

# Test
cargo +nightly test --features alloc,std

# Test no_std (core crates only)
cargo +nightly test -p mantis-core -p mantis-types -p mantis-queue --no-default-features

# Lint
cargo +nightly clippy --all-targets --features alloc,std -- -D warnings

# Format
cargo +nightly fmt --all

# Supply chain audit
cargo deny check

# Miri (undefined behavior detection)
cargo +nightly miri test -p mantis-queue
```

## `no_std` Rules

Core crates (`mantis-core`, `mantis-types`, `mantis-queue`, `mantis-platform`) are `#![no_std]` by default:

- No heap allocation in hot paths after initialization
- No panics in hot paths — use `Result` or error enum returns
- `std` and `alloc` are optional features

## Unsafe Policy

All unsafe code must follow the project's [unsafe policy](docs/UNSAFE.md):

- Unsafe code lives in `raw` submodules only
- Crate roots deny unsafe: `#![deny(unsafe_code)]`
- Every `unsafe` block requires a `// SAFETY:` comment explaining the invariant, the guarantee, and the failure mode
- Miri runs on every PR

## Benchmarks

```bash
# Run benchmarks
cargo bench --bench spsc

# With native CPU optimizations
RUSTFLAGS='-C target-cpu=native' cargo bench --bench spsc

# Including external contenders (rtrb, crossbeam, rigtorp)
cargo bench --bench spsc --features bench-contenders
```

Benchmark protocol:
- Never claim "fastest" without a published, reproducible benchmark
- All benchmarks export JSON to `target/` for cross-hardware comparison
- External contenders are behind the `bench-contenders` feature flag
- Same workload shapes across all implementations for fair comparison

## Commits

- Imperative mood, 72-character subject line limit
- One logical change per commit
- Run `cargo +nightly fmt --all && cargo +nightly clippy --all-targets --features alloc,std -- -D warnings && cargo +nightly test --features alloc,std` before committing

## Pull Requests

- Use feature branches — never push directly to `main`
- PRs should describe what the code does, not the journey to get there
- Keep PRs focused on a single concern

## Reporting Issues

Use [GitHub Issues](https://github.com/mantis-sdk/mantis/issues) for bug reports and feature requests. For security vulnerabilities, see [SECURITY.md](SECURITY.md).
