#!/usr/bin/env bash
set -euo pipefail

# perf_flamegraph.sh — Flamegraph generation from perf data
# Usage: ./perf_flamegraph.sh <bench-command...>
#
# Supports two flamegraph backends:
#   1. cargo-flamegraph / flamegraph CLI tool
#   2. Brendan Gregg's FlameGraph scripts (stackcollapse-perf.pl + flamegraph.pl)

if [[ $# -eq 0 ]]; then
    echo "Usage: $0 <bench-command...>"
    echo ""
    echo "Example: $0 cargo bench --bench spsc"
    echo ""
    echo "Records perf data with DWARF call graphs and generates"
    echo "a flamegraph SVG in the results/ directory."
    echo ""
    echo "Supports:"
    echo "  - 'flamegraph' CLI tool (cargo install flamegraph)"
    echo "  - Brendan Gregg's FlameGraph scripts (set FLAMEGRAPH_DIR)"
    exit 1
fi

if ! command -v perf &>/dev/null; then
    echo "ERROR: 'perf' not found. Install linux-tools-\$(uname -r) or perf."
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="${SCRIPT_DIR}/../results"
mkdir -p "$RESULTS_DIR"

TIMESTAMP=$(date +%Y%m%d_%H%M%S)
PERF_DATA="${RESULTS_DIR}/perf-flamegraph-${TIMESTAMP}.data"
SVG_OUT="${RESULTS_DIR}/flamegraph-${TIMESTAMP}.svg"

echo "=== Mantis perf flamegraph ==="
echo "Command: $*"
echo ""

# --- Method 1: flamegraph CLI ---
if command -v flamegraph &>/dev/null; then
    echo "Using 'flamegraph' CLI tool"
    flamegraph -o "$SVG_OUT" -- "$@"
    echo ""
    echo "Flamegraph written to: $SVG_OUT"
    exit 0
fi

# --- Method 2: Brendan Gregg's FlameGraph scripts ---
FLAMEGRAPH_DIR="${FLAMEGRAPH_DIR:-}"

# Try common locations if not set
if [[ -z "$FLAMEGRAPH_DIR" ]]; then
    for candidate in \
        "$HOME/FlameGraph" \
        "/opt/FlameGraph" \
        "/usr/local/share/FlameGraph" \
        "/usr/share/FlameGraph"; do
        if [[ -f "$candidate/flamegraph.pl" ]]; then
            FLAMEGRAPH_DIR="$candidate"
            break
        fi
    done
fi

if [[ -n "$FLAMEGRAPH_DIR" ]] && [[ -f "$FLAMEGRAPH_DIR/flamegraph.pl" ]]; then
    echo "Using FlameGraph scripts from: $FLAMEGRAPH_DIR"
    echo ""

    echo "-- Recording perf data --"
    perf record -g --call-graph dwarf -o "$PERF_DATA" -- "$@"

    echo ""
    echo "-- Generating flamegraph --"
    perf script -i "$PERF_DATA" \
        | "$FLAMEGRAPH_DIR/stackcollapse-perf.pl" \
        | "$FLAMEGRAPH_DIR/flamegraph.pl" \
        > "$SVG_OUT"

    echo "Flamegraph written to: $SVG_OUT"
    exit 0
fi

# --- Fallback: just record, user can process later ---
echo "WARNING: No flamegraph tool found."
echo "  Install one of:"
echo "    cargo install flamegraph"
echo "    git clone https://github.com/brendangregg/FlameGraph ~/FlameGraph"
echo "  Or set FLAMEGRAPH_DIR=/path/to/FlameGraph"
echo ""
echo "Recording perf data anyway..."

perf record -g --call-graph dwarf -o "$PERF_DATA" -- "$@"

echo ""
echo "Perf data saved to: $PERF_DATA"
echo "Generate flamegraph manually with:"
echo "  perf script -i $PERF_DATA | stackcollapse-perf.pl | flamegraph.pl > flamegraph.svg"
exit 0
