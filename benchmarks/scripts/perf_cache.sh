#!/usr/bin/env bash
set -euo pipefail

# perf_cache.sh — Cache-line contention analysis via perf c2c
# Usage: ./perf_cache.sh <bench-command...>

if [[ $# -eq 0 ]]; then
    echo "Usage: $0 <bench-command...>"
    echo ""
    echo "Example: $0 cargo bench --bench spsc"
    echo ""
    echo "Records cache-to-cache (c2c) events and reports false sharing"
    echo "and cache-line contention."
    exit 1
fi

if ! command -v perf &>/dev/null; then
    echo "ERROR: 'perf' not found. Install linux-tools-\$(uname -r) or perf."
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="${SCRIPT_DIR}/../results"
mkdir -p "$RESULTS_DIR"

PERF_DATA="${RESULTS_DIR}/perf-c2c.data"

echo "=== Mantis perf c2c — cache-line contention ==="
echo "Command: $*"
echo ""

echo "-- Recording c2c events --"
perf c2c record -o "$PERF_DATA" -- "$@"

echo ""
echo "-- Generating c2c report --"
perf c2c report -i "$PERF_DATA" --stdio

echo ""
echo "Raw data saved to: $PERF_DATA"
