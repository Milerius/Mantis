# mantis-verify

Formal verification and property-based testing for the Mantis SDK.

`std`-only tooling crate.

## Verification Layers

```
┌───────────────────────────────────────────┐
│            Kani Proofs (spsc_proofs)       │
│  Bounded model checking over ALL possible  │
│  push/pop sequences up to length N         │
│  Proves: FIFO, capacity, no data loss,     │
│          index safety                      │
└───────────────────────────────────────────┘
┌───────────────────────────────────────────┐
│       Bolero Property Tests (spsc_props)   │
│  Random push/pop sequences, checks:       │
│  - FIFO ordering                           │
│  - len() == pushed - popped                │
│  - push succeeds when len < capacity       │
│  - pop succeeds when len > 0               │
└───────────────────────────────────────────┘
┌───────────────────────────────────────────┐
│     Differential Tests (spsc_diff)         │
│  Same sequences on multiple presets,       │
│  verifies identical output:                │
│  - SpscRing vs SpscRingInstrumented        │
│  - SpscRing vs SpscRingHeap               │
└───────────────────────────────────────────┘
```

## Running

### Property + differential tests

```bash
cargo test -p mantis-verify
```

### Kani proofs (requires kani-verifier)

```bash
cargo install --locked kani-verifier
cargo kani setup
cargo kani -p mantis-verify
```

Kani proofs are `#[cfg(kani)]` gated and run in nightly CI.

## Tests

| Module | Tests | Method |
|---|---|---|
| `spsc_props` | 4 | Bolero property-based |
| `spsc_diff` | 3 | Differential (fixed + Bolero + heap) |
| `spsc_proofs` | 4 | Kani bounded model checking |

### Kani Proofs

| Proof | Property | Bound |
|---|---|---|
| `fifo_ordering_proof` | Popped values match pushed order | 8 ops, capacity 4 |
| `capacity_invariant_proof` | Never exceeds capacity | 5 ops, capacity 4 |
| `no_data_loss_proof` | All pushed items are retrievable | 3 pushes + drain |
| `index_safety_proof` | Indices never exceed storage | 8 ops, capacity 4 |
