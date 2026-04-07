#!/usr/bin/env bash
set -euo pipefail

# perf_profile.sh — Comprehensive perf stat profiling
# Usage: ./perf_profile.sh <bench-command...>

if [[ $# -eq 0 ]]; then
    echo "Usage: $0 <bench-command...>"
    echo ""
    echo "Example: $0 cargo bench --bench spsc"
    echo ""
    echo "Runs perf stat with a comprehensive set of hardware counters"
    echo "covering cycles, cache, branch, and TLB events."
    exit 1
fi

if ! command -v perf &>/dev/null; then
    echo "ERROR: 'perf' not found. Install linux-tools-\$(uname -r) or perf."
    exit 1
fi

EVENTS=$(cat <<'EVTS'
cycles,
instructions,
cache-references,
cache-misses,
L1-dcache-loads,
L1-dcache-load-misses,
L1-dcache-stores,
LLC-loads,
LLC-load-misses,
LLC-stores,
LLC-store-misses,
branch-loads,
branch-misses,
dTLB-loads,
dTLB-load-misses,
context-switches,
cpu-migrations
EVTS
)

# Remove newlines for the -e argument
EVENTS=$(echo "$EVENTS" | tr -d '\n' | tr -s ' ')

echo "=== Mantis perf stat — comprehensive profile ==="
echo "Command: $*"
echo ""

exec perf stat \
    --per-thread \
    -e "$EVENTS" \
    -- "$@"
