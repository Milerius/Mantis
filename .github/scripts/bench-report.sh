#!/usr/bin/env bash
set -euo pipefail

# Generate a comparison-style markdown benchmark report.
# Usage: bench-report.sh <linux-json> <macos-json>
# Outputs markdown to stdout.

linux_json="${1:?usage: bench-report.sh <linux-json> <macos-json>}"
macos_json="${2:?usage: bench-report.sh <linux-json> <macos-json>}"

# Normalize workload names into (impl, pattern, element) and extract metrics.
# Output: JSON array of {impl, pattern, element, ns_per_op, ...}
normalize='
[.results[] | {
  workload,
  impl: (
    if .workload | startswith("spsc/inline/") then "mantis/inline"
    elif .workload | startswith("copy/") then "mantis/copy"
    elif .workload | startswith("general/") then "mantis/general"
    elif .workload | startswith("spsc/rtrb/") then "rtrb"
    elif .workload | startswith("spsc/crossbeam/") then "crossbeam"
    elif .workload | startswith("spsc/rigtorp/") then "rigtorp-cpp"
    else "other"
    end
  ),
  pattern: (
    if (.workload | test("single_item/|single/")) then "single"
    elif (.workload | test("burst_100/|burst/100/")) then "burst_100"
    elif (.workload | test("burst_1000/|burst/1000/")) then "burst_1000"
    elif (.workload | test("batch/100/")) then "batch_100"
    elif (.workload | test("batch/1000/")) then "batch_1000"
    elif (.workload | test("full_drain/")) then "full_drain"
    else "other"
    end
  ),
  element: (.workload | split("/") | last),
  ns_per_op: (.ns_per_op | . * 100 | round / 100),
  cycles: (.cycles_per_op // null | if . == null then null elif . < 0.1 then 0 else (. * 10 | round / 10) end),
  insns: (.instructions_per_op // null | if . == null then null else round end),
  bmiss: (.branch_misses_per_op // null | if . == null then null elif . < 0.1 then 0 else (. * 10 | round / 10) end)
}]
'

# Build a comparison table for a given pattern.
# Args: $1=json_file $2=pattern $3=title
render_comparison() {
  local json="$1" pattern="$2" title="$3"
  local data impls elements

  data=$(jq -r --arg p "$pattern" "$normalize | map(select(.pattern == \$p))" "$json")
  elements=$(echo "$data" | jq -r '[.[].element] | unique | .[]')
  impls=$(echo "$data" | jq -r '[.[].impl] | unique | .[]')

  # Skip if no data for this pattern
  if [ -z "$elements" ]; then
    return
  fi

  # Build header
  local header="| Element |"
  local separator="|:--------|"
  for impl in $impls; do
    header="$header $impl |"
    separator="$separator------:|"
  done

  echo "#### $title"
  echo ""
  echo "$header"
  echo "$separator"

  # Build rows
  for elem in $elements; do
    local row="| \`$elem\` |"
    # Find the best (lowest) ns/op for this element
    local best
    best=$(echo "$data" | jq -r --arg e "$elem" \
      '[.[] | select(.element == $e) | .ns_per_op] | min')

    for impl in $impls; do
      local cell
      cell=$(echo "$data" | jq -r --arg i "$impl" --arg e "$elem" \
        '.[] | select(.impl == $i and .element == $e) | .ns_per_op // empty' 2>/dev/null)
      if [ -z "$cell" ]; then
        row="$row - |"
      else
        # Bold the best value with trophy emoji
        if [ "$cell" = "$best" ]; then
          row="$row **${cell}** 🏆 |"
        else
          row="$row $cell |"
        fi
      fi
    done
    echo "$row"
  done
  echo ""
}

# Build an insns/op comparison table for a given pattern.
render_insns_comparison() {
  local json="$1" pattern="$2" title="$3"
  local data impls elements has_any

  data=$(jq -r --arg p "$pattern" "$normalize | map(select(.pattern == \$p))" "$json")
  elements=$(echo "$data" | jq -r '[.[].element] | unique | .[]')
  impls=$(echo "$data" | jq -r '[.[].impl] | unique | .[]')

  # Check if any insns data exists
  has_any=$(echo "$data" | jq -r '[.[].insns | select(. != null)] | length')
  if [ "$has_any" = "0" ] || [ -z "$elements" ]; then
    return
  fi

  local header="| Element |"
  local separator="|:--------|"
  for impl in $impls; do
    header="$header $impl |"
    separator="$separator------:|"
  done

  echo "#### $title"
  echo ""
  echo "$header"
  echo "$separator"

  for elem in $elements; do
    local row="| \`$elem\` |"
    local best
    best=$(echo "$data" | jq -r --arg e "$elem" \
      '[.[] | select(.element == $e) | .insns | select(. != null)] | min // empty')

    for impl in $impls; do
      local cell
      cell=$(echo "$data" | jq -r --arg i "$impl" --arg e "$elem" \
        '.[] | select(.impl == $i and .element == $e) | .insns // empty' 2>/dev/null)
      if [ -z "$cell" ] || [ "$cell" = "null" ]; then
        row="$row - |"
      elif [ -n "$best" ] && [ "$cell" = "$best" ]; then
        row="$row **${cell}** 🏆 |"
      else
        row="$row $cell |"
      fi
    done
    echo "$row"
  done
  echo ""
}

# Render full detailed table in a collapsible section.
render_full_table() {
  local json="$1"

  echo "| Workload | ns/op | p50 | p99 | cycles | insns | bmiss | l1d | llc |"
  echo "|:---------|------:|----:|----:|-------:|------:|------:|----:|----:|"

  jq -r '.results[] |
    [
      .workload,
      (.ns_per_op | . * 100 | round / 100 | tostring),
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

# Render one platform section.
render_platform() {
  local json="$1" label="$2"
  local cpu arch compiler

  if [ ! -f "$json" ]; then
    echo "*${label} benchmark results not available.*"
    echo ""
    return
  fi

  cpu=$(jq -r '.cpu' "$json")
  arch=$(jq -r '.arch' "$json")
  compiler=$(jq -r '.compiler' "$json")

  echo "**CPU:** \`${cpu}\` | **Arch:** \`${arch}\` | **Compiler:** \`${compiler}\`"
  echo ""

  echo "##### Latency (ns/op, lower is better)"
  echo ""
  render_comparison "$json" "single" "Single Push+Pop"
  render_comparison "$json" "burst_100" "Burst 100"
  render_comparison "$json" "burst_1000" "Burst 1000"
  render_comparison "$json" "batch_100" "Batch 100"
  render_comparison "$json" "batch_1000" "Batch 1000"
  render_comparison "$json" "full_drain" "Full Drain"

  echo "##### Instructions per Op (lower is better)"
  echo ""
  render_insns_comparison "$json" "single" "Single Push+Pop"
  render_insns_comparison "$json" "burst_100" "Burst 100"

  echo "<details>"
  echo "<summary>Full results (all fields)</summary>"
  echo ""
  render_full_table "$json"
  echo ""
  echo "</details>"
}

commit_sha="${GITHUB_SHA:-$(git rev-parse --short HEAD)}"

cat <<HEADER
## Benchmark Report

<sub>Commit: \`${commit_sha}\`</sub>

HEADER

echo "<details open>"
echo "<summary><strong>Linux</strong></summary>"
echo ""
render_platform "$linux_json" "Linux"
echo "</details>"
echo ""

echo "<details open>"
echo "<summary><strong>macOS</strong></summary>"
echo ""
render_platform "$macos_json" "macOS"
echo "</details>"
