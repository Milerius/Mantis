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
        let cap = ring.capacity();
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
        }
    }
}
