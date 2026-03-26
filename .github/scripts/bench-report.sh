#!/usr/bin/env bash
set -euo pipefail

# Generate a markdown benchmark report from bench-report-spsc.json files.
# Usage: bench-report.sh <linux-json> <macos-json>
# Outputs markdown to stdout.

linux_json="${1:?usage: bench-report.sh <linux-json> <macos-json>}"
macos_json="${2:?usage: bench-report.sh <linux-json> <macos-json>}"

render_table() {
  local json="$1"
  local cpu arch compiler

  cpu=$(jq -r '.cpu' "$json")
  arch=$(jq -r '.arch' "$json")
  compiler=$(jq -r '.compiler' "$json")

  echo "**CPU:** \`${cpu}\` | **Arch:** \`${arch}\` | **Compiler:** \`${compiler}\`"
  echo ""
  echo "| Workload | ns/op | ops/s | p50 ns | p99 ns | cycles | insns | bmiss | l1d | llc |"
  echo "|:---------|------:|------:|-------:|-------:|-------:|------:|------:|----:|----:|"

  jq -r '.results[] |
    [
      .workload,
      (.ns_per_op | . * 100 | round / 100 | tostring),
      (.ops_per_sec | round | tostring),
      (.p50_ns | . * 10 | round / 10 | tostring),
      (.p99_ns | . * 10 | round / 10 | tostring),
      (.cycles_per_op // null | if . == null then "-" elif . < 0.1 then "<0.1" else (. * 10 | round / 10 | tostring) end),
      (.instructions_per_op // null | if . == null then "-" else (. | round | tostring) end),
      (.branch_misses_per_op // null | if . == null then "-" elif . < 0.1 then "<0.1" else (. * 10 | round / 10 | tostring) end),
      (.l1_misses_per_op // null | if . == null then "-" elif . == 0 then "0" else (. * 10 | round / 10 | tostring) end),
      (.llc_misses_per_op // null | if . == null then "-" elif . == 0 then "0" else (. * 10 | round / 10 | tostring) end)
    ] | "| " + join(" | ") + " |"
  ' "$json"
}

commit_sha="${GITHUB_SHA:-$(git rev-parse --short HEAD)}"

cat <<HEADER
## Benchmark Report

<sub>Commit: \`${commit_sha}\`</sub>

HEADER

echo "<details open>"
echo "<summary><strong>Linux</strong></summary>"
echo ""
if [ -f "$linux_json" ]; then
  render_table "$linux_json"
else
  echo "*Linux benchmark results not available.*"
fi
echo ""
echo "</details>"
echo ""

echo "<details open>"
echo "<summary><strong>macOS</strong></summary>"
echo ""
if [ -f "$macos_json" ]; then
  render_table "$macos_json"
else
  echo "*macOS benchmark results not available.*"
fi
echo ""
echo "</details>"
