#!/usr/bin/env bash
set -euo pipefail

# ASM inspection for Mantis hot-path functions.
#
# Uses `cargo-show-asm` against the asm_shim example to produce
# inspectable assembly from the *actual* crate code — no hardcoded
# source, no external APIs.
#
# Usage:
#   ./scripts/check-asm.sh                  # inspect all hot functions
#   ./scripts/check-asm.sh --baseline       # save as baseline for diffs
#   ./scripts/check-asm.sh --symbol <name>  # inspect a specific symbol
#
# Prerequisites: cargo install cargo-show-asm
#
# Output: target/asm/*.s (or target/asm/baseline/*.s with --baseline)

ASM_DIR="target/asm"
BASELINE_DIR="$ASM_DIR/baseline"
OUTPUT_DIR="$ASM_DIR"
CRATE="mantis-queue"
EXAMPLE="asm_shim"

# Functions exported by the asm_shim example
SYMBOLS=(
    "asm_shim::spsc_push_u64"
    "asm_shim::spsc_pop_u64"
    "asm_shim::spsc_push_bytes64"
    "asm_shim::spsc_pop_bytes64"
    "asm_shim::spsc_copy_push_u64"
    "asm_shim::spsc_copy_pop_u64"
    "asm_shim::spsc_copy_push_batch_u64"
    "asm_shim::spsc_copy_pop_batch_u64"
)

SINGLE_SYMBOL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --baseline)
            OUTPUT_DIR="$BASELINE_DIR"
            shift
            ;;
        --symbol)
            SINGLE_SYMBOL="$2"
            shift 2
            ;;
        *)
            echo "Usage: $0 [--baseline] [--symbol <name>]" >&2
            exit 1
            ;;
    esac
done

# Check cargo-show-asm is installed
if ! cargo asm --version &>/dev/null; then
    echo "ERROR: cargo-show-asm not installed." >&2
    echo "Install: cargo install cargo-show-asm" >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

# If a single symbol was requested, just show it
if [[ -n "$SINGLE_SYMBOL" ]]; then
    cargo asm -p "$CRATE" --example "$EXAMPLE" "$SINGLE_SYMBOL"
    exit 0
fi

# List available symbols
echo "Available symbols:"
cargo asm -p "$CRATE" --example "$EXAMPLE" 2>&1 | grep "asm_shim::" || true
echo ""

# Extract each symbol
for sym in "${SYMBOLS[@]}"; do
    # Short name for the file (strip "asm_shim::" prefix)
    short="${sym#asm_shim::}"
    outfile="$OUTPUT_DIR/${short}.s"

    echo "Extracting: $sym"
    if cargo asm -p "$CRATE" --example "$EXAMPLE" "$sym" > "$outfile" 2>/dev/null; then
        lines=$(wc -l < "$outfile" | tr -d ' ')
        echo "  -> $outfile ($lines lines)"
    else
        echo "  WARNING: failed to extract '$sym'" >&2
        rm -f "$outfile"
    fi
done

# Diff against baseline if it exists
if [[ -d "$BASELINE_DIR" && "$OUTPUT_DIR" != "$BASELINE_DIR" ]]; then
    echo ""
    echo "=== Diff against baseline ==="
    changed=0
    for f in "$OUTPUT_DIR"/*.s; do
        name=$(basename "$f")
        base="$BASELINE_DIR/$name"
        if [[ -f "$base" ]]; then
            DIFF=$(diff --unified=3 "$base" "$f" || true)
            if [[ -n "$DIFF" ]]; then
                old_count=$(wc -l < "$base" | tr -d ' ')
                new_count=$(wc -l < "$f" | tr -d ' ')
                echo "CHANGED: $name ($old_count -> $new_count lines)"
                echo "$DIFF"
                changed=1
            else
                echo "UNCHANGED: $name"
            fi
        else
            echo "NEW: $name (no baseline)"
        fi
    done
    if [[ $changed -eq 0 ]]; then
        echo "No ASM changes detected."
    fi
fi

echo ""
echo "Done. Output in $OUTPUT_DIR/"
[[ "$OUTPUT_DIR" != "$BASELINE_DIR" ]] && \
    echo "Use --baseline to save current output as baseline."
