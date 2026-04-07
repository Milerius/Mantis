#!/usr/bin/env bash
set -euo pipefail

# prepare_system.sh — Prepare system for stable benchmarking (requires root)
# Sets CPU governor to performance, disables turbo boost, sets perf_event_paranoid.

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "ERROR: This script is Linux-only."
    exit 1
fi

if [[ "$(id -u)" -ne 0 ]]; then
    echo "ERROR: This script must be run as root."
    echo "Usage: sudo $0"
    exit 1
fi

RED='\033[0;31m'
GRN='\033[0;32m'
BLD='\033[1m'
RST='\033[0m'

section() { printf "\n${BLD}--- %s ---${RST}\n" "$1"; }

# --- CPU Governor ---
section "CPU Governor"

gov_path_base="/sys/devices/system/cpu"
if ls "${gov_path_base}"/cpu*/cpufreq/scaling_governor &>/dev/null; then
    echo "Before:"
    for g in "${gov_path_base}"/cpu*/cpufreq/scaling_governor; do
        cpu=$(echo "$g" | grep -oP 'cpu\d+')
        printf "  %s: %s\n" "$cpu" "$(cat "$g")"
    done

    for g in "${gov_path_base}"/cpu*/cpufreq/scaling_governor; do
        echo "performance" > "$g"
    done

    echo "After:"
    for g in "${gov_path_base}"/cpu*/cpufreq/scaling_governor; do
        cpu=$(echo "$g" | grep -oP 'cpu\d+')
        printf "  %s: %s\n" "$cpu" "$(cat "$g")"
    done
else
    echo "  No cpufreq governor interface found (VM or fixed-frequency CPU)"
fi

# --- Turbo Boost ---
section "Turbo Boost"

turbo_set=0

# Intel pstate
if [[ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]]; then
    printf "  Before (intel_pstate/no_turbo): %s\n" "$(cat /sys/devices/system/cpu/intel_pstate/no_turbo)"
    echo 1 > /sys/devices/system/cpu/intel_pstate/no_turbo
    printf "  After  (intel_pstate/no_turbo): %s\n" "$(cat /sys/devices/system/cpu/intel_pstate/no_turbo)"
    turbo_set=1
fi

# Generic cpufreq boost
if [[ -f /sys/devices/system/cpu/cpufreq/boost ]]; then
    printf "  Before (cpufreq/boost): %s\n" "$(cat /sys/devices/system/cpu/cpufreq/boost)"
    echo 0 > /sys/devices/system/cpu/cpufreq/boost
    printf "  After  (cpufreq/boost): %s\n" "$(cat /sys/devices/system/cpu/cpufreq/boost)"
    turbo_set=1
fi

if [[ "$turbo_set" -eq 0 ]]; then
    echo "  No turbo boost interface found"
fi

# --- perf_event_paranoid ---
section "perf_event_paranoid"

paranoid_path="/proc/sys/kernel/perf_event_paranoid"
if [[ -f "$paranoid_path" ]]; then
    printf "  Before: %s\n" "$(cat "$paranoid_path")"
    echo 1 > "$paranoid_path"
    printf "  After:  %s\n" "$(cat "$paranoid_path")"
else
    echo "  perf_event_paranoid not found"
fi

# --- Done ---
printf "\n${GRN}System prepared for benchmarking.${RST}\n"
printf "Run ./check_system.sh to verify.\n"
