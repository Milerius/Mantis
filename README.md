# Mantis

[![CI](https://github.com/mantis-sdk/mantis/actions/workflows/ci.yml/badge.svg)](https://github.com/mantis-sdk/mantis/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/mantis-sdk/mantis/graph/badge.svg)](https://codecov.io/gh/mantis-sdk/mantis)

A modular, `no_std`-first Rust foundation for low-latency financial systems across centralized and decentralized markets, with first-class replay, verification, and performance tooling.

## Status

Phase 0 — Infrastructure bootstrap. Building the SPSC ring and core primitives.

## Quick Start

```bash
# Install tooling
just setup

# Build
cargo build --all-features

# Test
cargo test --all-features

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Benchmark
cargo bench

# Layout inspection
cargo run -p mantis-layout
```

## Architecture

See [CLAUDE.md](CLAUDE.md) for the full development guide.

## License

Apache-2.0
