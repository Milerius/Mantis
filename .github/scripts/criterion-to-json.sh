#!/usr/bin/env bash
set -euo pipefail

# Convert Criterion benchmark estimates to a simple JSON array.
# Usage: criterion-to-json.sh <bench-name>
# Outputs: target/bench-report-<bench-name>.json
#
# Reads from target/criterion/<group>/<bench>/new/estimates.json

bench_name="${1:?usage: criterion-to-json.sh <bench-name>}"
criterion_dir="target/criterion"
output="target/bench-report-${bench_name}.json"

if [ ! -d "$criterion_dir" ]; then
  echo "No criterion results found at $criterion_dir" >&2
  exit 1
fi

# Collect all estimates into a JSON array
results="[]"

for est in "$criterion_dir"/*/new/estimates.json "$criterion_dir"/*/*/new/estimates.json; do
  [ -f "$est" ] || continue

  # Extract group/bench name from path:
  #   target/criterion/checked_add/FixedI64<6>/new/estimates.json -> checked_add/FixedI64<6>
  #   target/criterion/checked_mul_trunc/D=6/new/estimates.json -> checked_mul_trunc/D=6
  rel="${est#"$criterion_dir/"}"
  workload="${rel%/new/estimates.json}"

  # Extract point estimate (nanoseconds)
  ns_per_op=$(jq -r '.mean.point_estimate' "$est")

  results=$(echo "$results" | jq \
    --arg w "$workload" \
    --argjson ns "$ns_per_op" \
    '. + [{"workload": $w, "ns_per_op": ($ns | . * 100 | round / 100)}]')
done

# Wrap in the same schema as spsc report
cpu="unknown"
arch="unknown"
compiler="unknown"

if [ "$(uname)" = "Darwin" ]; then
  cpu=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "unknown")
  arch=$(uname -m)
elif [ -f /proc/cpuinfo ]; then
  cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs)
  arch=$(uname -m)
fi
compiler=$(rustc +nightly --version 2>/dev/null | head -1 || echo "unknown")

jq -n \
  --arg cpu "$cpu" \
  --arg arch "$arch" \
  --arg compiler "$compiler" \
  --argjson results "$results" \
  '{cpu: $cpu, arch: $arch, compiler: $compiler, results: $results}' > "$output"

echo "Wrote $(echo "$results" | jq length) benchmarks to $output" >&2
