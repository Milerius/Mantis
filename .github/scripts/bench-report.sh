#!/usr/bin/env bash
set -euo pipefail

# Generate a comparison-style markdown benchmark report.
# Usage: bench-report.sh <linux-json> <macos-json>
# Outputs markdown to stdout.

linux_seqlock="${1:?usage: bench-report.sh <linux-seqlock> <macos-seqlock> [<linux-fixed> <macos-fixed>] [<linux-market-state> <macos-market-state>]}"
macos_seqlock="${2:?}"
linux_fixed="${3:-}"
macos_fixed="${4:-}"
linux_market_state="${5:-}"
macos_market_state="${6:-}"

# Render grouped benchmark results from Criterion JSON report.
# Args: $1=json_file $2=label $3=suite_name (e.g. "fixed-point", "seqlock")
render_grouped_platform() {
  local json="$1" label="$2" suite="${3:-benchmark}"

  if [ ! -f "$json" ]; then
    echo "*${label} ${suite} benchmark results not available.*"
    echo ""
    return
  fi

  local cpu arch compiler
  cpu=$(jq -r '.cpu' "$json")
  arch=$(jq -r '.arch' "$json")
  compiler=$(jq -r '.compiler' "$json")

  echo "**CPU:** \`${cpu}\` | **Arch:** \`${arch}\` | **Compiler:** \`${compiler}\`"
  echo ""

  # Group by benchmark category (first path component)
  local groups
  groups=$(jq -r '[.results[].workload | split("/")[0]] | unique | .[]' "$json")

  for group in $groups; do
    echo "#### ${group}"
    echo ""
    echo "| Variant | ns/op |"
    echo "|:--------|------:|"

    jq -r --arg g "$group" '
      .results
      | map(select(.workload | startswith($g + "/")))
      | sort_by(.ns_per_op)
      | .[]
      | "| `" + (.workload | split("/")[1:] | join("/")) + "` | " + (.ns_per_op | tostring) + " |"
    ' "$json"

    echo ""
  done
}

commit_sha="${GITHUB_SHA:-$(git rev-parse --short HEAD)}"

cat <<HEADER
## Benchmark Report

<sub>Commit: \`${commit_sha}\`</sub>

HEADER

# Seqlock benchmarks
if [ -n "$linux_seqlock" ] || [ -n "$macos_seqlock" ]; then
  echo "### Sequence Lock (mantis-seqlock)"
  echo ""

  if [ -n "$linux_seqlock" ]; then
    echo "<details open>"
    echo "<summary><strong>Linux</strong></summary>"
    echo ""
    render_grouped_platform "$linux_seqlock" "Linux" "seqlock"
    echo "</details>"
    echo ""
  fi

  if [ -n "$macos_seqlock" ]; then
    echo "<details open>"
    echo "<summary><strong>macOS</strong></summary>"
    echo ""
    render_grouped_platform "$macos_seqlock" "macOS" "seqlock"
    echo "</details>"
  fi
fi

# Fixed-point benchmarks (optional)
if [ -n "$linux_fixed" ] || [ -n "$macos_fixed" ]; then
  echo ""
  echo "### Fixed-Point Arithmetic (mantis-fixed)"
  echo ""

  if [ -n "$linux_fixed" ]; then
    echo "<details open>"
    echo "<summary><strong>Linux</strong></summary>"
    echo ""
    render_grouped_platform "$linux_fixed" "Linux" "fixed-point"
    echo "</details>"
    echo ""
  fi

  if [ -n "$macos_fixed" ]; then
    echo "<details open>"
    echo "<summary><strong>macOS</strong></summary>"
    echo ""
    render_grouped_platform "$macos_fixed" "macOS" "fixed-point"
    echo "</details>"
  fi
fi

# Market-state benchmarks (optional)
if [ -n "$linux_market_state" ] || [ -n "$macos_market_state" ]; then
  echo ""
  echo "### Market-State Engine (mantis-market-state)"
  echo ""

  if [ -n "$linux_market_state" ]; then
    echo "<details open>"
    echo "<summary><strong>Linux</strong></summary>"
    echo ""
    render_grouped_platform "$linux_market_state" "Linux" "market-state"
    echo "</details>"
    echo ""
  fi

  if [ -n "$macos_market_state" ]; then
    echo "<details open>"
    echo "<summary><strong>macOS</strong></summary>"
    echo ""
    render_grouped_platform "$macos_market_state" "macOS" "market-state"
    echo "</details>"
  fi
fi
