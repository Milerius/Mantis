#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

usage() {
    cat <<EOF
Usage: $(basename "$0") user@host --producer-core P --consumer-core C [extra args...]

Deploy benchmarks to a remote machine, run them, and retrieve results.

Arguments:
  user@host           Remote SSH target (required, first positional arg)
  --producer-core P   CPU core for producer thread (required)
  --consumer-core C   CPU core for consumer thread (required)
  [extra args]        Additional arguments forwarded to run_bench.sh

Examples:
  $(basename "$0") bench@server1 --producer-core 9 --consumer-core 10
  $(basename "$0") root@10.0.0.5 --producer-core 2 --consumer-core 3 --messages 5000000
EOF
    exit 1
}

if [[ $# -lt 1 ]]; then
    usage
fi

REMOTE_HOST="$1"
shift

# Validate that it looks like user@host or hostname
if [[ "$REMOTE_HOST" == --* ]]; then
    echo "Error: first argument must be user@host, got '$REMOTE_HOST'"
    usage
fi

# Remaining args are forwarded to run_bench.sh
BENCH_ARGS=("$@")

REMOTE_DIR="~/mantis-benchmarks"

echo "=== Deploy & Run: $REMOTE_HOST ==="
echo "Remote directory: $REMOTE_DIR"
echo "Bench args: ${BENCH_ARGS[*]}"
echo ""

# ── Step 1: Deploy sources ───────────────────────────────────────────────────
echo "--- Step 1/4: Deploying benchmark sources to $REMOTE_HOST ---"
rsync -avz --delete \
    --exclude 'results/' \
    --exclude 'rust/target/' \
    --exclude 'cpp/build/' \
    --exclude 'cpp/vendor/SPSCQueue/' \
    --exclude 'cpp/vendor/SPSC-Queue/' \
    --exclude '.DS_Store' \
    "$BENCH_DIR/" \
    "$REMOTE_HOST:$REMOTE_DIR/"

# ── Step 2: Setup (if needed) and run ────────────────────────────────────────
echo ""
echo "--- Step 2/4: Running benchmarks on $REMOTE_HOST ---"
# shellcheck disable=SC2029
ssh "$REMOTE_HOST" "cd $REMOTE_DIR && chmod +x scripts/*.sh && bash scripts/run_bench.sh ${BENCH_ARGS[*]}"

# ── Step 3: Retrieve results ────────────────────────────────────────────────
echo ""
echo "--- Step 3/4: Retrieving results from $REMOTE_HOST ---"
mkdir -p "$BENCH_DIR/results"
rsync -avz \
    "$REMOTE_HOST:$REMOTE_DIR/results/" \
    "$BENCH_DIR/results/"

# ── Step 4: Local comparison report ──────────────────────────────────────────
echo ""
echo "--- Step 4/4: Generating local comparison report ---"
if [[ -f "$SCRIPT_DIR/compare.py" ]]; then
    python3 "$SCRIPT_DIR/compare.py" "$BENCH_DIR/results" | tee "$BENCH_DIR/results/comparison.md"
else
    echo "WARNING: compare.py not found, skipping local comparison"
fi

echo ""
echo "=== Deploy & Run complete ==="
echo "Results in: $BENCH_DIR/results/"
