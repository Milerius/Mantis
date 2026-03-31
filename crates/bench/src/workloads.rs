//! Standardized workload shapes for SPSC ring benchmarks.

use mantis_queue::{SpscRing, SpscRingCopy};

/// Push 1, pop 1, repeat `n` times. Measures per-op latency.
pub fn single_item<const N: usize>(ring: &mut SpscRing<u64, N>, n: usize) {
    for i in 0..n {
        let _ = ring.try_push(i as u64);
        let _ = ring.try_pop();
    }
}

/// Push `burst_size` items, then pop all, repeat `rounds` times.
pub fn burst<const N: usize>(ring: &mut SpscRing<u64, N>, burst_size: usize, rounds: usize) {
    for round in 0..rounds {
        for i in 0..burst_size {
            if ring.try_push((round * burst_size + i) as u64).is_err() {
                break;
            }
        }
        while ring.try_pop().is_ok() {}
    }
}

/// Fill ring completely, drain completely, repeat `rounds` times.
pub fn full_drain<const N: usize>(ring: &mut SpscRing<u64, N>, rounds: usize) {
    let cap = ring.capacity();
    for round in 0..rounds {
        for i in 0..cap {
            let _ = ring.try_push((round * cap + i) as u64);
        }
        while ring.try_pop().is_ok() {}
    }
}

/// Single push+pop for copy ring.
pub fn single_item_copy<T: Copy + Send + Default, const N: usize>(
    ring: &mut SpscRingCopy<T, N>,
    values: &[T],
) {
    let mut out = T::default();
    for val in values {
        let _ = ring.push(val);
        let _ = ring.pop(&mut out);
    }
}

/// Burst of single push+pop for copy ring.
pub fn burst_copy<T: Copy + Send + Default, const N: usize>(
    ring: &mut SpscRingCopy<T, N>,
    values: &[T],
    burst_size: usize,
) {
    let mut out = T::default();
    for chunk in values.chunks(burst_size) {
        for val in chunk {
            let _ = ring.push(val);
        }
        for _ in 0..chunk.len() {
            let _ = ring.pop(&mut out);
        }
    }
}

/// Batch push + batch pop for copy ring.
pub fn batch_copy<T: Copy + Send + Default, const N: usize>(
    ring: &mut SpscRingCopy<T, N>,
    values: &[T],
    batch_size: usize,
) {
    let mut out = vec![T::default(); batch_size];
    for chunk in values.chunks(batch_size) {
        let pushed = ring.push_batch(chunk);
        let _ = ring.pop_batch(&mut out[..pushed]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_item_workload() {
        let mut ring = SpscRing::<u64, 64>::new();
        single_item(&mut ring, 100);
    }

    #[test]
    fn burst_workload() {
        let mut ring = SpscRing::<u64, 128>::new();
        burst(&mut ring, 50, 100);
    }

    #[test]
    fn full_drain_workload() {
        let mut ring = SpscRing::<u64, 64>::new();
        full_drain(&mut ring, 10);
    }

    #[test]
    fn single_item_copy_workload() {
        let mut ring = SpscRingCopy::<u64, 64>::new();
        let vals: Vec<u64> = (0..100).collect();
        single_item_copy(&mut ring, &vals);
    }

    #[test]
    fn burst_copy_workload() {
        let mut ring = SpscRingCopy::<u64, 128>::new();
        let vals: Vec<u64> = (0..100).collect();
        burst_copy(&mut ring, &vals, 50);
    }

    #[test]
    fn batch_copy_workload() {
        let mut ring = SpscRingCopy::<u64, 128>::new();
        let vals: Vec<u64> = (0..100).collect();
        batch_copy(&mut ring, &vals, 50);
    }
}
