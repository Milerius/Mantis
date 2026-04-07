#!/usr/bin/env bash
set -euo pipefail

# perf_branch.sh — Branch misprediction analysis
# Usage: ./perf_branch.sh <bench-command...>

if [[ $# -eq 0 ]]; then
    echo "Usage: $0 <bench-command...>"
    echo ""
    echo "Example: $0 cargo bench --bench spsc"
    echo ""
    echo "Runs perf stat with branch-related counters to identify"
    echo "branch misprediction hotspots."
    exit 1
fi

if ! command -v perf &>/dev/null; then
    echo "ERROR: 'perf' not found. Install linux-tools-\$(uname -r) or perf."
    exit 1
fi

EVENTS=$(cat <<'EVTS'
cycles,
instructions,
branches,
branch-misses,
branch-loads,
branch-load-misses
EVTS
)

EVENTS=$(echo "$EVENTS" | tr -d '\n' | tr -s ' ')

echo "=== Mantis perf stat — branch misprediction ==="
echo "Command: $*"
echo ""

exec perf stat \
    --per-thread \
    -e "$EVENTS" \
    -- "$@"
