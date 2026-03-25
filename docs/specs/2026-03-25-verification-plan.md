# Verification — Implementation Plan (3 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add formal verification (Kani proofs), property-based testing (Bolero), and differential testing for the SPSC ring buffer in `mantis-verify`.

**Architecture:** Kani bounded model checking proves FIFO ordering, capacity invariants, and index safety for small bounds. Bolero property tests check invariants over arbitrary push/pop sequences. Differential tests run the same sequences on all presets and verify identical output.

**Tech Stack:** Kani 0.x, Bolero 0.11, `mantis-queue` presets.

**Spec:** `docs/specs/2026-03-25-spsc-ring-bench-design.md` — Section 4 (Verification).

**Prerequisite:** Plan 1 (SPSC Ring Buffer) must be complete.

---

## File Structure

### New files

| File | Responsibility |
|---|---|
| `crates/verify/src/spsc_proofs.rs` | Kani bounded model checking proofs |
| `crates/verify/src/spsc_props.rs` | Bolero property-based tests |
| `crates/verify/src/spsc_diff.rs` | Differential testing across presets |

### Modified files

| File | Changes |
|---|---|
| `crates/verify/src/lib.rs` | Add modules, remove placeholder |
| `crates/verify/Cargo.toml` | Add `mantis-queue` features |

---

## Task 1: Bolero property-based tests

**Files:**
- Create: `crates/verify/src/spsc_props.rs`
- Modify: `crates/verify/src/lib.rs`
- Modify: `crates/verify/Cargo.toml`

- [ ] **Step 1: Update Cargo.toml**

Ensure `mantis-queue` has `alloc` feature:

```toml
[dependencies]
mantis-queue = { workspace = true, features = ["alloc"] }
```

- [ ] **Step 2: Write property tests**

In `crates/verify/src/spsc_props.rs`:

```rust
//! Bolero property-based tests for SPSC ring invariants.

#[cfg(test)]
mod tests {
    use bolero::check;
    use mantis_queue::{QueueError, SpscRing};

    /// Arbitrary push/pop sequences maintain FIFO ordering.
    #[test]
    fn fifo_ordering() {
        check!()
            .with_type::<Vec<bool>>()
            .for_each(|ops| {
                let mut ring = SpscRing::<u64, 16>::new();
                let mut pushed = Vec::new();
                let mut popped = Vec::new();
                let mut push_val = 0u64;

                for &is_push in ops.iter() {
                    if is_push {
                        if ring.try_push(push_val).is_ok() {
                            pushed.push(push_val);
                            push_val += 1;
                        }
                    } else if let Ok(val) = ring.try_pop() {
                        popped.push(val);
                    }
                }
                // Drain remaining
                while let Ok(val) = ring.try_pop() {
                    popped.push(val);
                }

                // Popped must be a prefix of pushed
                assert_eq!(
                    popped,
                    pushed[..popped.len()],
                    "FIFO violation"
                );
            });
    }

    /// count_pushed - count_popped == ring.len() invariant.
    #[test]
    fn len_invariant() {
        check!()
            .with_type::<Vec<bool>>()
            .for_each(|ops| {
                let mut ring = SpscRing::<u64, 16>::new();
                let mut count_pushed = 0usize;
                let mut count_popped = 0usize;
                let mut push_val = 0u64;

                for &is_push in ops.iter() {
                    if is_push {
                        if ring.try_push(push_val).is_ok() {
                            count_pushed += 1;
                            push_val += 1;
                        }
                    } else if ring.try_pop().is_ok() {
                        count_popped += 1;
                    }

                    assert_eq!(
                        ring.len(),
                        count_pushed - count_popped,
                        "len invariant violated"
                    );
                }
            });
    }

    /// Ring never reports full when len < capacity.
    #[test]
    fn not_full_when_under_capacity() {
        check!()
            .with_type::<Vec<bool>>()
            .for_each(|ops| {
                let mut ring = SpscRing::<u64, 8>::new();
                let cap = ring.capacity();
                let mut push_val = 0u64;

                for &is_push in ops.iter() {
                    if is_push {
                        if ring.len() < cap {
                            assert!(
                                ring.try_push(push_val).is_ok(),
                                "push failed with len {} < cap {}",
                                ring.len(),
                                cap,
                            );
                            push_val += 1;
                        }
                    } else {
                        let _ = ring.try_pop();
                    }
                }
            });
    }

    /// Ring never reports empty when len > 0.
    #[test]
    fn not_empty_when_has_items() {
        check!()
            .with_type::<Vec<bool>>()
            .for_each(|ops| {
                let mut ring = SpscRing::<u64, 8>::new();
                let mut push_val = 0u64;

                for &is_push in ops.iter() {
                    if is_push {
                        let _ = ring.try_push(push_val);
                        push_val += 1;
                    } else if ring.len() > 0 {
                        assert!(
                            ring.try_pop().is_ok(),
                            "pop failed when ring had items",
                        );
                    }
                }
            });
    }
}
```

- [ ] **Step 3: Wire up module in `lib.rs`**

Replace `crates/verify/src/lib.rs`. Note: only add `spsc_props` for now — `spsc_diff` and `spsc_proofs` will be added in Tasks 2 and 3 respectively.

```rust
//! Formal verification and property-based testing for the Mantis SDK.
//!
//! Contains kani proof harnesses, bolero property tests,
//! and differential testing utilities.

#![deny(unsafe_code)]

mod spsc_props;
```

- [ ] **Step 4: Run property tests**

Run: `cargo test -p mantis-verify`
Expected: PASS — all 4 property tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/verify/src/spsc_props.rs crates/verify/src/lib.rs crates/verify/Cargo.toml
git commit -m "test(verify): add bolero property tests for SPSC ring"
```

---

## Task 2: Differential testing across presets

**Files:**
- Create: `crates/verify/src/spsc_diff.rs`

- [ ] **Step 1: Write differential tests**

```rust
//! Differential testing: run identical sequences on all presets,
//! verify identical output.

#[cfg(test)]
mod tests {
    use mantis_queue::{SpscRingInstrumented, SpscRing};

    #[cfg(feature = "alloc")]
    use mantis_queue::SpscRingHeap;

    /// Run the same push/pop sequence on two rings, assert same output.
    fn compare_sequences(ops: &[bool]) {
        let mut ring_a = SpscRing::<u64, 16>::new();
        let mut ring_b = SpscRingInstrumented::<u64, 16>::new();

        let mut push_val = 0u64;

        for &is_push in ops {
            if is_push {
                let res_a = ring_a.try_push(push_val);
                let res_b = ring_b.try_push(push_val);
                assert_eq!(
                    res_a.is_ok(),
                    res_b.is_ok(),
                    "push divergence at val {push_val}"
                );
                if res_a.is_ok() {
                    push_val += 1;
                }
            } else {
                let res_a = ring_a.try_pop();
                let res_b = ring_b.try_pop();
                assert_eq!(res_a, res_b, "pop divergence");
            }
        }

        // Drain and compare
        loop {
            let a = ring_a.try_pop();
            let b = ring_b.try_pop();
            assert_eq!(a, b, "drain divergence");
            if a.is_err() {
                break;
            }
        }
    }

    #[test]
    fn portable_vs_instrumented_fixed() {
        // Deterministic sequence: push 5, pop 3, push 5, pop all
        let mut ops = Vec::new();
        for _ in 0..5 {
            ops.push(true);
        }
        for _ in 0..3 {
            ops.push(false);
        }
        for _ in 0..5 {
            ops.push(true);
        }
        for _ in 0..10 {
            ops.push(false);
        }
        compare_sequences(&ops);
    }

    #[test]
    fn portable_vs_instrumented_bolero() {
        bolero::check!()
            .with_type::<Vec<bool>>()
            .for_each(|ops| {
                compare_sequences(ops);
            });
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn portable_vs_heap() {
        let ops: Vec<bool> = (0..200)
            .map(|i| i % 3 != 0) // 2 pushes, 1 pop
            .collect();

        let mut ring_inline = SpscRing::<u64, 16>::new();
        let mut ring_heap = SpscRingHeap::<u64>::with_capacity(16);

        let mut push_val = 0u64;
        for &is_push in &ops {
            if is_push {
                let a = ring_inline.try_push(push_val);
                let b = ring_heap.try_push(push_val);
                assert_eq!(a.is_ok(), b.is_ok());
                if a.is_ok() {
                    push_val += 1;
                }
            } else {
                let a = ring_inline.try_pop();
                let b = ring_heap.try_pop();
                assert_eq!(a, b);
            }
        }
    }
}
```

- [ ] **Step 2: Add `spsc_diff` module to `lib.rs`**

Add to `crates/verify/src/lib.rs`:

```rust
mod spsc_diff;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p mantis-verify --all-features`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/verify/src/spsc_diff.rs crates/verify/src/lib.rs
git commit -m "test(verify): add differential testing across SPSC presets"
```

---

## Task 3: Kani bounded model checking proofs

**Files:**
- Create: `crates/verify/src/spsc_proofs.rs`

- [ ] **Step 1: Write Kani proofs**

```rust
//! Kani bounded model checking proofs for SPSC ring invariants.
//!
//! These proofs verify correctness for all possible push/pop
//! sequences up to a bounded length on small-capacity rings.

#[cfg(kani)]
mod proofs {
    use mantis_queue::SpscRing;

    /// FIFO ordering: for all push/pop sequences of length <= 8
    /// on capacity 4, output order matches input order.
    #[kani::proof]
    #[kani::unwind(10)]
    fn fifo_ordering_proof() {
        let mut ring = SpscRing::<u8, 4>::new();
        let mut pushed: [u8; 8] = [0; 8];
        let mut popped: [u8; 8] = [0; 8];
        let mut push_count = 0usize;
        let mut pop_count = 0usize;
        let mut next_val: u8 = 0;

        for _ in 0..8 {
            let do_push: bool = kani::any();
            if do_push {
                if ring.try_push(next_val).is_ok() {
                    pushed[push_count] = next_val;
                    push_count += 1;
                    next_val = next_val.wrapping_add(1);
                }
            } else if let Ok(val) = ring.try_pop() {
                popped[pop_count] = val;
                pop_count += 1;
            }
        }

        // Verify FIFO: popped values match pushed values in order
        for i in 0..pop_count {
            kani::assert(
                popped[i] == pushed[i],
                "FIFO ordering violation",
            );
        }
    }

    /// Capacity invariant: ring never accepts more than capacity
    /// items without a pop.
    #[kani::proof]
    #[kani::unwind(6)]
    fn capacity_invariant_proof() {
        let mut ring = SpscRing::<u8, 4>::new();
        let cap = ring.capacity(); // 3 (capacity - 1 sentinel)
        let mut count = 0usize;

        for _ in 0..5 {
            let do_push: bool = kani::any();
            if do_push {
                if ring.try_push(0).is_ok() {
                    count += 1;
                }
            } else if ring.try_pop().is_ok() {
                count -= 1;
            }

            kani::assert(count <= cap, "exceeded capacity");
        }
    }

    /// No data loss: items pushed are always retrievable.
    #[kani::proof]
    #[kani::unwind(8)]
    fn no_data_loss_proof() {
        let mut ring = SpscRing::<u8, 4>::new();
        let mut push_count = 0usize;
        let mut pop_count = 0usize;

        // Push some items
        for _ in 0..3 {
            if ring.try_push(42).is_ok() {
                push_count += 1;
            }
        }

        // Pop all
        while ring.try_pop().is_ok() {
            pop_count += 1;
        }

        kani::assert(
            push_count == pop_count,
            "data loss: pushed != popped",
        );
    }

    /// Index safety: wrapped indices never exceed storage bounds.
    #[kani::proof]
    #[kani::unwind(10)]
    fn index_safety_proof() {
        let mut ring = SpscRing::<u8, 4>::new();

        for _ in 0..8 {
            let do_push: bool = kani::any();
            if do_push {
                let _ = ring.try_push(0);
            } else {
                let _ = ring.try_pop();
            }
            // If we get here without panic, indices were valid.
            // The debug_assert in slot_ptr catches OOB.
        }
    }
}
```

- [ ] **Step 2: Add `spsc_proofs` module to `lib.rs`**

Add to `crates/verify/src/lib.rs`:

```rust
#[cfg(kani)]
mod spsc_proofs;
```

- [ ] **Step 3: Verify syntax compiles (non-kani)**

Run: `cargo check -p mantis-verify`
Expected: PASS (kani module is `cfg(kani)` gated)

- [ ] **Step 4: Run kani proofs (if kani installed)**

Run: `cargo kani -p mantis-verify`
Expected: All 4 proofs verified successfully

Note: If kani is not installed locally, this will be validated by the CI nightly job.

- [ ] **Step 5: Commit**

```bash
git add crates/verify/src/spsc_proofs.rs crates/verify/src/lib.rs
git commit -m "verify(queue): add kani bounded model checking proofs"
```

---

## Task 4: Update `docs/PROGRESS.md`

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: Update Phase 1 Section 1.1 checkboxes**

Mark completed verification items:
- [x] Kani bounded model checking proofs
- [x] Bolero property-based tests
- [x] Differential testing across strategy variants

Update crate status:
| `mantis-verify` | Active | std | ~10 | — | 4 kani proofs, 6 bolero |

- [ ] **Step 2: Commit**

```bash
git add docs/PROGRESS.md
git commit -m "docs: update PROGRESS.md with verification completion"
```

---

## Summary

| Task | What | Commit |
|---|---|---|
| 1 | Bolero property tests (4 properties) | `test(verify): bolero property tests` |
| 2 | Differential testing (3 comparisons) | `test(verify): differential testing` |
| 3 | Kani proofs (4 proofs) | `verify(queue): kani proofs` |
| 4 | Progress doc update | `docs: update PROGRESS.md` |

**Total: 4 tasks, ~4 commits, 4 kani proofs + 7 property/differential tests.**

**Deferred from spec:**
- **rtrb as external oracle** (spec Section 4.5): deferred until Plan 2 adds rtrb as a dependency. Can be added as a differential test once `bench-contenders` feature is available.
- **Two-thread interleaving property test** (spec Section 4.7): deferred — requires `std::thread::spawn` and split handles, which adds complexity. The 10M-item stress test in Plan 1 covers two-thread correctness. A bolero-driven version can be added later.
