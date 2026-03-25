# Mantis — Task Runner
# Run `just --list` to see all available commands.

# Install all development tools
setup:
    cargo install cargo-deny --locked
    cargo install cargo-careful --locked
    cargo install cargo-mutants --locked
    cargo install cargo-llvm-cov --locked
    cargo install cargo-fuzz --locked
    cargo install cargo-criterion --locked
    cargo install just --locked
    @echo "Note: kani requires separate install — see docs/specs/"
    @echo "All tools installed."

# Verify all tools are available
check-tools:
    @echo "Checking tools..."
    cargo deny --version
    cargo careful --version
    cargo mutants --version
    cargo llvm-cov --version
    @echo "All tools available."

# Format all code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all --check

# Lint all code
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --all-features

# Run no_std tests
test-no-std:
    cargo test -p mantis-core -p mantis-types -p mantis-queue --no-default-features

# Run miri on unsafe-containing crates
miri:
    cargo +nightly miri test -p mantis-queue

# Run cargo careful
careful:
    cargo +nightly careful test

# Run cargo deny
deny:
    cargo deny check

# Build docs
doc:
    cargo doc --no-deps --all-features --open

# Run all CI checks locally
ci: fmt-check lint test test-no-std deny doc

# Run benchmarks
bench:
    cargo bench

# Run layout inspector
layout:
    cargo run -p mantis-layout

# Coverage report
coverage:
    cargo llvm-cov --all-features --html
    @echo "Report: target/llvm-cov/html/index.html"

# Coverage with branch coverage
coverage-branch:
    cargo llvm-cov --all-features --branch --html
    @echo "Report: target/llvm-cov/html/index.html"
