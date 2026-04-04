//! Kani bounded model checking proofs for mantis-seqlock.

#[cfg(kani)]
mod proofs {
    use mantis_seqlock::SeqLock;

    /// Prove: sequence counter is always even after store completes.
    #[kani::proof]
    fn version_even_after_store() {
        let mut lock = SeqLock::<u64>::new(0);
        let val: u64 = kani::any();
        lock.store(val);
        let v = lock.version();
        assert!(v & 1 == 0, "version must be even after store");
    }

    /// Prove: version increments by exactly 2 per store.
    #[kani::proof]
    fn version_increments_by_two() {
        let mut lock = SeqLock::<u64>::new(0);
        let v0 = lock.version();
        let val: u64 = kani::any();
        lock.store(val);
        let v1 = lock.version();
        assert!(v1 == v0.wrapping_add(2));
    }

    /// Prove: load returns the stored value in single-threaded context.
    #[kani::proof]
    fn load_returns_stored_value() {
        let mut lock = SeqLock::<u64>::new(0);
        let val: u64 = kani::any();
        lock.store(val);
        let loaded = lock.load();
        assert!(loaded == val);
    }

    /// Prove: consecutive stores, load returns the last one.
    #[kani::proof]
    #[kani::unwind(3)]
    fn load_returns_latest() {
        let mut lock = SeqLock::<u64>::new(0);
        let val1: u64 = kani::any();
        let val2: u64 = kani::any();
        lock.store(val1);
        lock.store(val2);
        let loaded = lock.load();
        assert!(loaded == val2);
    }
}
