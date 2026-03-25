//! Standardized workload shapes for SPSC ring benchmarks.

use mantis_queue::SpscRing;

/// Push 1, pop 1, repeat `n` times. Measures per-op latency.
pub fn single_item<const N: usize>(ring: &mut SpscRing<u64, N>, n: usize) {
    for i in 0..n {
        let _ = ring.try_push(i as u64);
        let _ = ring.try_pop();
    }
}

/// Push `burst_size` items, then pop all, repeat `rounds` times.
pub fn burst<const N: usize>(
    ring: &mut SpscRing<u64, N>,
    burst_size: usize,
    rounds: usize,
) {
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
}
