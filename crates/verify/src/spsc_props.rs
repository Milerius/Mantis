//! Bolero property-based tests for SPSC ring invariants.

#[cfg(test)]
mod tests {
    use bolero::check;
    use mantis_queue::SpscRing;

    /// Arbitrary push/pop sequences maintain FIFO ordering.
    #[test]
    fn fifo_ordering() {
        check!().with_type::<Vec<bool>>().for_each(|ops| {
            let mut ring = SpscRing::<u64, 16>::new();
            let mut pushed = Vec::new();
            let mut popped = Vec::new();
            let mut push_val = 0u64;

            for &is_push in ops {
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
            assert_eq!(popped, pushed[..popped.len()], "FIFO violation");
        });
    }

    /// `count_pushed - count_popped == ring.len()` invariant.
    #[test]
    fn len_invariant() {
        check!().with_type::<Vec<bool>>().for_each(|ops| {
            let mut ring = SpscRing::<u64, 16>::new();
            let mut count_pushed = 0usize;
            let mut count_popped = 0usize;
            let mut push_val = 0u64;

            for &is_push in ops {
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
        check!().with_type::<Vec<bool>>().for_each(|ops| {
            let mut ring = SpscRing::<u64, 8>::new();
            let cap = ring.capacity();
            let mut push_val = 0u64;

            for &is_push in ops {
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
        check!().with_type::<Vec<bool>>().for_each(|ops| {
            let mut ring = SpscRing::<u64, 8>::new();
            let mut push_val = 0u64;

            for &is_push in ops {
                if is_push {
                    let _ = ring.try_push(push_val);
                    push_val += 1;
                } else if !ring.is_empty() {
                    assert!(ring.try_pop().is_ok(), "pop failed when ring had items",);
                }
            }
        });
    }

    /// Batch push followed by batch pop preserves FIFO ordering.
    #[test]
    fn copy_batch_fifo_ordering() {
        bolero::check!().with_type::<Vec<u8>>().for_each(|data| {
            if data.is_empty() {
                return;
            }

            let mut ring = mantis_queue::SpscRingCopy::<u8, 256>::new();
            let pushed = ring.push_batch(data);
            let mut out = vec![0u8; pushed];
            let popped = ring.pop_batch(&mut out);

            assert_eq!(pushed, popped);
            assert_eq!(&data[..pushed], &out[..popped]);
        });
    }

    /// Batch push never exceeds capacity.
    #[test]
    fn copy_batch_respects_capacity() {
        bolero::check!().with_type::<Vec<u8>>().for_each(|data| {
            let mut ring = mantis_queue::SpscRingCopy::<u8, 16>::new();
            let pushed = ring.push_batch(data);
            assert!(pushed <= ring.capacity());
            assert!(pushed <= data.len());
        });
    }
}
