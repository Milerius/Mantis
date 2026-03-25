#!/usr/bin/env bash
set -euo pipefail

# Godbolt ASM inspection for Mantis hot functions.
#
# Usage: ./scripts/check-asm.sh [--baseline]
#
# Sends push/pop implementations to Godbolt Compiler Explorer API
# for x86_64 and aarch64, saves output to target/asm/.
# With --baseline, saves to target/asm/baseline/ for future diffs.

GODBOLT_API="https://godbolt.org/api"
ASM_DIR="target/asm"
BASELINE_DIR="$ASM_DIR/baseline"

if [[ "${1:-}" == "--baseline" ]]; then
    OUTPUT_DIR="$BASELINE_DIR"
else
    OUTPUT_DIR="$ASM_DIR"
fi

mkdir -p "$OUTPUT_DIR"

# Extract push/pop source for compilation
PUSH_SOURCE=$(cat <<'RUST'
use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::Cell;

#[repr(align(128))]
struct Padded<T>(T);

impl<T> core::ops::Deref for Padded<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.0 }
}

pub struct Ring {
    head: Padded<AtomicUsize>,
    tail: Padded<AtomicUsize>,
    tail_cached: Padded<Cell<usize>>,
    buf: *mut u64,
    mask: usize,
}

#[inline(never)]
#[no_mangle]
pub unsafe fn ring_push(ring: &Ring, value: u64) -> bool {
    let head = ring.head.load(Ordering::Relaxed);
    let next = (head + 1) & ring.mask;
    if next == ring.tail_cached.get() {
        let tail = ring.tail.load(Ordering::Acquire);
        ring.tail_cached.set(tail);
        if next == tail { return false; }
    }
    *ring.buf.add(head) = value;
    ring.head.store(next, Ordering::Release);
    true
}
RUST
)

# Compile for x86_64
echo "Compiling for x86_64..."
RESPONSE=$(curl -s -X POST "$GODBOLT_API/compiler/nightly/compile" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "$(jq -n \
        --arg source "$PUSH_SOURCE" \
        '{
            source: $source,
            options: {
                userArguments: "-C opt-level=3 -C target-cpu=x86-64-v3",
                filters: { intel: true, directives: true, commentOnly: true, labels: true }
            }
        }')")

echo "$RESPONSE" | jq -r '.asm[]?.text // empty' > "$OUTPUT_DIR/ring_push_x86_64.s"
echo "Saved: $OUTPUT_DIR/ring_push_x86_64.s"

# Compile for aarch64
echo "Compiling for aarch64..."
RESPONSE=$(curl -s -X POST "$GODBOLT_API/compiler/nightly/compile" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json" \
    -d "$(jq -n \
        --arg source "$PUSH_SOURCE" \
        '{
            source: $source,
            options: {
                userArguments: "-C opt-level=3 --target aarch64-unknown-linux-gnu",
                filters: { directives: true, commentOnly: true, labels: true }
            }
        }')")

echo "$RESPONSE" | jq -r '.asm[]?.text // empty' > "$OUTPUT_DIR/ring_push_aarch64.s"
echo "Saved: $OUTPUT_DIR/ring_push_aarch64.s"

# Diff against baseline if exists
if [[ -d "$BASELINE_DIR" && "$OUTPUT_DIR" != "$BASELINE_DIR" ]]; then
    echo ""
    echo "=== Diff against baseline ==="
    for f in "$OUTPUT_DIR"/*.s; do
        base="$BASELINE_DIR/$(basename "$f")"
        if [[ -f "$base" ]]; then
            DIFF=$(diff "$base" "$f" || true)
            if [[ -n "$DIFF" ]]; then
                OLD_COUNT=$(wc -l < "$base" | tr -d ' ')
                NEW_COUNT=$(wc -l < "$f" | tr -d ' ')
                echo "CHANGED: $(basename "$f") ($OLD_COUNT -> $NEW_COUNT instructions)"
                echo "$DIFF"
            else
                echo "UNCHANGED: $(basename "$f")"
            fi
        fi
    done
fi

echo ""
echo "Done. Use --baseline to save current output as baseline."
