#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Defaults
PRODUCER_CORE=""
CONSUMER_CORE=""
MESSAGES=1000000
WARMUP=100000
RUNS=5

usage() {
    cat <<EOF
Usage: $(basename "$0") --producer-core P --consumer-core C [OPTIONS]

Options:
  --producer-core P   CPU core for producer thread (required)
  --consumer-core C   CPU core for consumer thread (required)
  --messages N        Number of messages per run (default: $MESSAGES)
  --warmup N          Number of warmup messages (default: $WARMUP)
  --runs N            Number of runs (default: $RUNS)
  -h, --help          Show this help
EOF
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --producer-core) PRODUCER_CORE="$2"; shift 2 ;;
        --consumer-core) CONSUMER_CORE="$2"; shift 2 ;;
        --messages)      MESSAGES="$2"; shift 2 ;;
        --warmup)        WARMUP="$2"; shift 2 ;;
        --runs)          RUNS="$2"; shift 2 ;;
        -h|--help)       usage ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

if [[ -z "$PRODUCER_CORE" || -z "$CONSUMER_CORE" ]]; then
    echo "Error: --producer-core and --consumer-core are required"
    usage
fi

RESULTS_DIR="$BENCH_DIR/results"
mkdir -p "$RESULTS_DIR"

# ── Step 1: System checks ────────────────────────────────────────────────────
echo "=== Step 1/7: System checks ==="
if [[ -x "$SCRIPT_DIR/check_system.sh" ]]; then
    # Run check_system.sh; warn on failure but don't abort
    "$SCRIPT_DIR/check_system.sh" || echo "WARNING: check_system.sh reported issues (continuing anyway)"
else
    echo "WARNING: check_system.sh not found, skipping system checks"
fi

# ── Step 2: Build Rust ───────────────────────────────────────────────────────
echo ""
echo "=== Step 2/7: Building Rust benchmark ==="
(cd "$BENCH_DIR/rust" && RUSTFLAGS='-C target-cpu=native' cargo +nightly build --release)

# ── Step 3: Build C++ ────────────────────────────────────────────────────────
echo ""
echo "=== Step 3/7: Building C++ benchmark ==="
(cd "$BENCH_DIR/cpp" && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build --parallel)

# ── Step 4: Run Rust benchmark ───────────────────────────────────────────────
echo ""
echo "=== Step 4/7: Running Rust benchmark ==="
"$BENCH_DIR/rust/target/release/mantis-spsc-bench" \
    --queue all \
    --producer-core "$PRODUCER_CORE" \
    --consumer-core "$CONSUMER_CORE" \
    --messages "$MESSAGES" \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --output-dir "$RESULTS_DIR"

# ── Step 5: Run C++ benchmark ───────────────────────────────────────────────
echo ""
echo "=== Step 5/7: Running C++ benchmark ==="
"$BENCH_DIR/cpp/build/spsc-bench" \
    --queue all \
    --producer-core "$PRODUCER_CORE" \
    --consumer-core "$CONSUMER_CORE" \
    --messages "$MESSAGES" \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --output-dir "$RESULTS_DIR"

# ── Step 6: Perf profiling (quick run) ───────────────────────────────────────
echo ""
echo "=== Step 6/7: Perf profiling (quick run) ==="
if [[ -x "$SCRIPT_DIR/perf_profile.sh" ]]; then
    "$SCRIPT_DIR/perf_profile.sh" \
        --binary "$BENCH_DIR/rust/target/release/mantis-spsc-bench" \
        --queue mantis-copy \
        --producer-core "$PRODUCER_CORE" \
        --consumer-core "$CONSUMER_CORE" \
        --messages 100000 \
        --runs 1 \
        --output-dir "$RESULTS_DIR" || echo "WARNING: perf profiling for mantis-copy failed"

    "$SCRIPT_DIR/perf_profile.sh" \
        --binary "$BENCH_DIR/cpp/build/spsc-bench" \
        --queue rigtorp \
        --producer-core "$PRODUCER_CORE" \
        --consumer-core "$CONSUMER_CORE" \
        --messages 100000 \
        --runs 1 \
        --output-dir "$RESULTS_DIR" || echo "WARNING: perf profiling for rigtorp failed"
else
    echo "WARNING: perf_profile.sh not found, skipping profiling"
fi

# ── Step 7: Comparison report ────────────────────────────────────────────────
echo ""
echo "=== Step 7/7: Generating comparison report ==="
python3 "$SCRIPT_DIR/compare.py" "$RESULTS_DIR" | tee "$RESULTS_DIR/comparison.md"

echo ""
echo "=== Benchmark complete ==="
echo "Results in: $RESULTS_DIR"
echo "Report:     $RESULTS_DIR/comparison.md"
