#!/usr/bin/env bash
set -euo pipefail

# check_system.sh — Pre-flight system validation for benchmarking
# Checks TSC, CPU isolation, governor, turbo boost, perf access, and cache topology.

RED='\033[0;31m'
YEL='\033[1;33m'
GRN='\033[0;32m'
RST='\033[0m'

PASS="${GRN}PASS${RST}"
WARN="${YEL}WARN${RST}"
FAIL="${RED}FAIL${RST}"

has_fail=0

pass() { printf "  [${PASS}] %s\n" "$1"; }
warn() { printf "  [${WARN}] %s\n" "$1"; }
fail() { printf "  [${FAIL}] %s\n" "$1"; has_fail=1; }

echo "=== Mantis Benchmark System Check ==="
echo ""

# --- Platform detection ---
OS="$(uname -s)"

# --- CPU model and topology ---
echo "-- CPU Info --"
if [[ "$OS" == "Linux" ]]; then
    if command -v lscpu &>/dev/null; then
        lscpu | grep -E '(Model name|Socket|Core|Thread|CPU\(s\)|NUMA|Cache)'
    else
        grep 'model name' /proc/cpuinfo | head -1
    fi
elif [[ "$OS" == "Darwin" ]]; then
    sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "Unknown CPU"
    echo "Cores: $(sysctl -n hw.physicalcpu 2>/dev/null || echo '?') physical, $(sysctl -n hw.logicalcpu 2>/dev/null || echo '?') logical"
    sysctl -n hw.l1dcachesize hw.l2cachesize hw.l3cachesize 2>/dev/null | paste - - - | awk '{printf "Cache: L1d=%s L2=%s L3=%s\n", $1, $2, $3}' || true
fi
echo ""

# --- TSC ---
echo "-- TSC (Time Stamp Counter) --"
if [[ "$OS" == "Linux" ]] && [[ -f /proc/cpuinfo ]]; then
    flags=$(grep -m1 '^flags' /proc/cpuinfo || echo "")
    if echo "$flags" | grep -q 'constant_tsc'; then
        pass "constant_tsc present"
    else
        fail "constant_tsc NOT found in CPU flags"
    fi
    if echo "$flags" | grep -q 'nonstop_tsc'; then
        pass "nonstop_tsc present"
    else
        fail "nonstop_tsc NOT found in CPU flags"
    fi
elif [[ "$OS" == "Darwin" ]]; then
    warn "TSC check not available on macOS (assumed invariant on modern Apple/Intel)"
else
    warn "Cannot check TSC on this platform"
fi
echo ""

# --- isolcpus ---
echo "-- CPU Isolation --"
if [[ "$OS" == "Linux" ]]; then
    cmdline=$(cat /proc/cmdline 2>/dev/null || echo "")
    if echo "$cmdline" | grep -q 'isolcpus'; then
        isolcpus=$(echo "$cmdline" | grep -oP 'isolcpus=\S+')
        pass "isolcpus configured: $isolcpus"
    else
        warn "isolcpus not set — consider isolating benchmark cores"
    fi
else
    warn "CPU isolation check not available on $OS"
fi
echo ""

# --- CPU governor ---
echo "-- CPU Governor --"
if [[ "$OS" == "Linux" ]]; then
    gov_path="/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"
    if [[ -f "$gov_path" ]]; then
        gov=$(cat "$gov_path")
        if [[ "$gov" == "performance" ]]; then
            pass "CPU governor = performance"
        else
            fail "CPU governor = $gov (should be 'performance')"
        fi
    else
        warn "CPU governor sysfs not found (might be fixed-frequency or VM)"
    fi
else
    warn "CPU governor check not available on $OS"
fi
echo ""

# --- Turbo boost ---
echo "-- Turbo Boost --"
if [[ "$OS" == "Linux" ]]; then
    turbo_checked=0
    # Intel pstate
    if [[ -f /sys/devices/system/cpu/intel_pstate/no_turbo ]]; then
        no_turbo=$(cat /sys/devices/system/cpu/intel_pstate/no_turbo)
        if [[ "$no_turbo" == "1" ]]; then
            pass "Turbo boost disabled (intel_pstate)"
        else
            fail "Turbo boost ENABLED (intel_pstate) — set no_turbo=1"
        fi
        turbo_checked=1
    fi
    # Generic cpufreq boost
    if [[ -f /sys/devices/system/cpu/cpufreq/boost ]]; then
        boost=$(cat /sys/devices/system/cpu/cpufreq/boost)
        if [[ "$boost" == "0" ]]; then
            pass "Turbo boost disabled (cpufreq/boost)"
        else
            fail "Turbo boost ENABLED (cpufreq/boost) — set boost=0"
        fi
        turbo_checked=1
    fi
    if [[ "$turbo_checked" == "0" ]]; then
        warn "Could not find turbo boost controls"
    fi
else
    warn "Turbo boost check not available on $OS"
fi
echo ""

# --- perf_event_paranoid ---
echo "-- perf_event_paranoid --"
if [[ "$OS" == "Linux" ]]; then
    paranoid_path="/proc/sys/kernel/perf_event_paranoid"
    if [[ -f "$paranoid_path" ]]; then
        val=$(cat "$paranoid_path")
        if [[ "$val" -le 1 ]]; then
            pass "perf_event_paranoid = $val"
        else
            fail "perf_event_paranoid = $val (should be <= 1)"
        fi
    else
        warn "perf_event_paranoid not found"
    fi
else
    warn "perf_event_paranoid check not available on $OS"
fi
echo ""

# --- L3 cache topology ---
echo "-- L3 Cache Topology --"
if [[ "$OS" == "Linux" ]]; then
    l3_index="/sys/devices/system/cpu/cpu0/cache"
    if [[ -d "$l3_index" ]]; then
        found_l3=0
        for idx in "$l3_index"/index*; do
            if [[ -f "$idx/level" ]] && [[ "$(cat "$idx/level")" == "3" ]]; then
                size=$(cat "$idx/size" 2>/dev/null || echo "?")
                shared=$(cat "$idx/shared_cpu_list" 2>/dev/null || echo "?")
                pass "L3 cache: ${size}, shared by CPUs: ${shared}"
                found_l3=1
            fi
        done
        if [[ "$found_l3" == "0" ]]; then
            warn "No L3 cache found in sysfs"
        fi
    else
        warn "Cache topology sysfs not found"
    fi
elif [[ "$OS" == "Darwin" ]]; then
    l3=$(sysctl -n hw.l3cachesize 2>/dev/null || echo "0")
    if [[ "$l3" -gt 0 ]]; then
        l3_mb=$((l3 / 1024 / 1024))
        pass "L3 cache: ${l3_mb} MB"
    else
        warn "No L3 cache reported"
    fi
else
    warn "Cache topology check not available on $OS"
fi
echo ""

# --- Summary ---
if [[ "$has_fail" -ne 0 ]]; then
    printf "${RED}System has FAILURES — benchmark results may be unreliable.${RST}\n"
    exit 1
else
    printf "${GRN}System looks good for benchmarking.${RST}\n"
    exit 0
fi
