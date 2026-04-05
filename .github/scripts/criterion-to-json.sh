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

# Search for estimates matching the bench name prefix (e.g. "seqlock/*" for seqlock bench,
# "market_state/*" for market_state bench).
#
# IMPORTANT: Each bench must be converted IMMEDIATELY after running, before the next
# bench clears target/criterion/. See bench.yml workflow comments.
#
# NO FALLBACK: if prefix doesn't match, we fail loudly rather than silently reading
# another bench's data. This prevents cross-contamination bugs.
shopt -s nullglob
matched_ests=()
# Criterion writes group/variant/new/estimates.json where group = bench_name with / → _
# Depths: group/new/estimates.json (no variant), group/variant/new/estimates.json (1 variant),
#          group/variant/subvar/new/estimates.json (2 variants)
for est in "$criterion_dir"/${bench_name}*/new/estimates.json \
           "$criterion_dir"/${bench_name}*/*/new/estimates.json \
           "$criterion_dir"/${bench_name}*/*/*/new/estimates.json; do
  matched_ests+=("$est")
done
shopt -u nullglob

if [ ${#matched_ests[@]} -eq 0 ]; then
  echo "WARNING: No criterion results found matching prefix '${bench_name}' in $criterion_dir" >&2
  echo "Available directories:" >&2
  ls -1 "$criterion_dir" 2>/dev/null | head -20 >&2 || true
  echo "Criterion group names must start with '${bench_name}' (the bench binary name)." >&2
  echo "Check your benchmark_group() names in the bench source file." >&2
  # Write empty result rather than reading wrong data
  jq -n --arg cpu "unknown" --arg arch "unknown" --arg compiler "unknown" \
    '{cpu: $cpu, arch: $arch, compiler: $compiler, results: []}' > "$output"
  echo "Wrote 0 benchmarks to $output (no matching data)" >&2
  exit 0
fi

for est in "${matched_ests[@]}"; do
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
