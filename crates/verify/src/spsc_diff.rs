//! Differential testing: run identical sequences on all presets,
//! verify identical output.

#[cfg(test)]
mod tests {
    use mantis_queue::{SpscRing, SpscRingHeap, SpscRingInstrumented};

    /// Run the same push/pop sequence on inline and instrumented rings.
    fn compare_inline_vs_instrumented(ops: &[bool]) {
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
        compare_inline_vs_instrumented(&ops);
    }

    #[test]
    fn portable_vs_instrumented_bolero() {
        bolero::check!()
            .with_type::<Vec<bool>>()
            .for_each(|ops| {
                compare_inline_vs_instrumented(ops);
            });
    }

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
