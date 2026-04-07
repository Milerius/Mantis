#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Mantis Benchmark Environment Setup ==="
echo ""

# ── Detect package manager ───────────────────────────────────────────────────
install_packages() {
    if command -v apt-get &>/dev/null; then
        echo "Detected apt-based system"
        sudo apt-get update
        sudo apt-get install -y cmake g++ linux-tools-common linux-tools-generic \
            linux-tools-"$(uname -r)" perf || true
    elif command -v dnf &>/dev/null; then
        echo "Detected dnf-based system"
        sudo dnf install -y cmake gcc-c++ perf
    elif command -v pacman &>/dev/null; then
        echo "Detected pacman-based system"
        sudo pacman -Sy --noconfirm cmake gcc perf
    else
        echo "WARNING: Unknown package manager. Please install cmake, g++, and perf manually."
    fi
}

# ── Step 1: System packages ──────────────────────────────────────────────────
echo "--- Step 1/7: Installing system packages ---"
install_packages

# ── Step 2: Rust nightly toolchain ───────────────────────────────────────────
echo ""
echo "--- Step 2/7: Installing Rust nightly toolchain ---"
if command -v rustup &>/dev/null; then
    rustup toolchain install nightly
    rustup default nightly
    echo "Rust nightly installed"
else
    echo "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi

# ── Step 3: Cargo tools ─────────────────────────────────────────────────────
echo ""
echo "--- Step 3/7: Installing cargo tools ---"
cargo install flamegraph 2>/dev/null || echo "flamegraph already installed or install failed"

# ── Step 4: Clone C++ vendor dependencies ────────────────────────────────────
echo ""
echo "--- Step 4/7: Cloning C++ vendor dependencies ---"
VENDOR_DIR="$BENCH_DIR/cpp/vendor"
mkdir -p "$VENDOR_DIR"

if [[ ! -d "$VENDOR_DIR/SPSCQueue/.git" ]]; then
    echo "Cloning rigtorp/SPSCQueue..."
    git clone --depth 1 https://github.com/rigtorp/SPSCQueue.git "$VENDOR_DIR/SPSCQueue"
else
    echo "SPSCQueue already cloned"
fi

if [[ ! -d "$VENDOR_DIR/SPSC-Queue/.git" ]]; then
    echo "Cloning drogalis/SPSC-Queue..."
    git clone --depth 1 https://github.com/drogalis/SPSC-Queue.git "$VENDOR_DIR/SPSC-Queue"
else
    echo "SPSC-Queue already cloned"
fi

# ── Step 5: Build Rust benchmark ─────────────────────────────────────────────
echo ""
echo "--- Step 5/7: Building Rust benchmark ---"
(cd "$BENCH_DIR/rust" && RUSTFLAGS='-C target-cpu=native' cargo +nightly build --release)

# ── Step 6: Build C++ benchmark ──────────────────────────────────────────────
echo ""
echo "--- Step 6/7: Building C++ benchmark ---"
(cd "$BENCH_DIR/cpp" && cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build --parallel)

# ── Step 7: System check ────────────────────────────────────────────────────
echo ""
echo "--- Step 7/7: Running system checks ---"
if [[ -x "$SCRIPT_DIR/check_system.sh" ]]; then
    "$SCRIPT_DIR/check_system.sh" || true
fi

# ── CPU topology / isolcpus recommendation ───────────────────────────────────
echo ""
echo "=== CPU Topology & Recommended isolcpus ==="
if command -v lscpu &>/dev/null; then
    echo ""
    echo "Online CPUs:"
    lscpu | grep -E "^CPU\(s\)|Thread|Core|Socket|NUMA|Model name" || true

    echo ""
    echo "Per-core mapping:"
    lscpu -p=CPU,CORE,SOCKET,NODE 2>/dev/null | grep -v '^#' | head -32 || true

    # Try to find two cores on the same socket for producer/consumer
    echo ""
    echo "Recommendation:"
    # Parse lscpu to find sibling cores on the same NUMA node
    CORES=$(lscpu -p=CPU,CORE,SOCKET 2>/dev/null | grep -v '^#' | sort -t, -k3,3n -k2,2n)
    if [[ -n "$CORES" ]]; then
        # Pick first two distinct physical cores from the same socket
        SOCKET0_CORES=$(echo "$CORES" | awk -F, '$3==0' | awk -F, '!seen[$2]++{print $1}' | head -2)
        CORE_A=$(echo "$SOCKET0_CORES" | head -1)
        CORE_B=$(echo "$SOCKET0_CORES" | tail -1)
        if [[ -n "$CORE_A" && -n "$CORE_B" && "$CORE_A" != "$CORE_B" ]]; then
            echo "  Add to kernel cmdline: isolcpus=$CORE_A,$CORE_B"
            echo "  Then run: ./run_bench.sh --producer-core $CORE_A --consumer-core $CORE_B"
        else
            echo "  Could not auto-detect suitable cores. Check lscpu output above."
        fi
    fi
else
    echo "lscpu not available (non-Linux system?). Skip topology analysis."
    echo "On macOS, pin cores manually based on sysctl hw.ncpu output."
fi

echo ""
echo "=== Setup complete ==="
