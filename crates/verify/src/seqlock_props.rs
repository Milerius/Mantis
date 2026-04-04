//! Bolero property-based tests for mantis-seqlock.

#[cfg(test)]
mod tests {
    use bolero::check;
    use mantis_seqlock::SeqLock;

    #[test]
    fn prop_store_load_roundtrip_u64() {
        check!().with_type::<u64>().for_each(|val| {
            let mut lock = SeqLock::<u64>::new(0);
            lock.store(*val);
            assert_eq!(lock.load(), *val);
        });
    }

    #[test]
    fn prop_store_load_roundtrip_array() {
        check!().with_type::<[u64; 4]>().for_each(|val| {
            let mut lock = SeqLock::<[u64; 4]>::new([0; 4]);
            lock.store(*val);
            assert_eq!(lock.load(), *val);
        });
    }

    #[test]
    fn prop_version_always_even() {
        check!().with_type::<Vec<u64>>().for_each(|vals| {
            let mut lock = SeqLock::<u64>::new(0);
            for v in vals {
                lock.store(*v);
                assert_eq!(lock.version() & 1, 0);
            }
        });
    }

    #[test]
    fn prop_version_monotonic() {
        check!().with_type::<Vec<u64>>().for_each(|vals| {
            let mut lock = SeqLock::<u64>::new(0);
            let mut prev = lock.version();
            for v in vals {
                lock.store(*v);
                let cur = lock.version();
                assert_eq!(cur, prev.wrapping_add(2), "version must increase by 2 per store");
                prev = cur;
            }
        });
    }
}
