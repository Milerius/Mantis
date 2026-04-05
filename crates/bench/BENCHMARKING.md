# Benchmarking Guide

## Naming Convention (CRITICAL)

Criterion `benchmark_group()` names **MUST** be prefixed with the bench binary name followed by `/`:

```rust
// In benches/market_state.rs:
c.benchmark_group("market_state/array_book");  // ✅ Correct
c.benchmark_group("array_book");               // ❌ WRONG — will not be collected

// In benches/fixed.rs:
c.benchmark_group("fixed/checked_add");        // ✅ Correct
c.benchmark_group("checked_add");              // ❌ WRONG — will not be collected

// In benches/seqlock.rs:
c.benchmark_group("seqlock/write");            // ✅ Correct
c.benchmark_group("write");                    // ❌ WRONG — will not be collected
```

### Why This Matters

Criterion writes results to `target/criterion/<group_with_slash_to_underscore>/`. The CI script
`criterion-to-json.sh` uses prefix matching (`${bench_name}*`) to collect only the results from
the bench that just ran. Without the prefix, results either:
- Don't get collected (empty report section)
- Get collected by a fallback that picks up **wrong bench data** (cross-contamination)

Both have happened in production. The fallback was removed — now missing prefixes produce an
explicit warning and empty results instead of silently wrong data.

## CI Pipeline

The bench workflow (`bench.yml`) runs each bench binary sequentially:

```
1. cargo bench --bench spsc         → custom JSON (not Criterion)
2. cargo bench --bench seqlock      → target/criterion/seqlock_*/
   criterion-to-json.sh seqlock     → target/bench-report-seqlock.json
   rm -rf target/criterion
3. cargo bench --bench fixed        → target/criterion/fixed_*/
   criterion-to-json.sh fixed       → target/bench-report-fixed.json
   rm -rf target/criterion
4. cargo bench --bench market_state → target/criterion/market_state_*/
   criterion-to-json.sh market_state → target/bench-report-market_state.json
```

**Key invariant:** Each `criterion-to-json.sh` runs IMMEDIATELY after its bench, BEFORE the
next `rm -rf target/criterion`. Moving the conversion after all benches will cause
cross-contamination (only the last bench's data survives).

## Adding a New Benchmark

1. Create `benches/<name>.rs`
2. All `benchmark_group()` calls must start with `<name>/`
3. Add `[[bench]]` entry in `Cargo.toml`
4. Add to `bench.yml`: run, convert, clear sequence (in order)
5. Add to `bench-report.sh`: render section
6. Test locally: `rm -rf target/criterion && cargo bench --bench <name> && bash .github/scripts/criterion-to-json.sh <name>`
7. Verify: `jq '.results[].workload' target/bench-report-<name>.json` — all should start with `<name>`

## Local Testing

```bash
# Test single bench
cargo +nightly bench --bench market_state

# Test full CI pipeline locally
rm -rf target/criterion
cargo +nightly bench --bench seqlock
bash .github/scripts/criterion-to-json.sh seqlock
rm -rf target/criterion
cargo +nightly bench --bench fixed
bash .github/scripts/criterion-to-json.sh fixed
rm -rf target/criterion
cargo +nightly bench --bench market_state
bash .github/scripts/criterion-to-json.sh market_state

# Verify no cross-contamination
jq '.results[].workload' target/bench-report-seqlock.json       # all seqlock_*
jq '.results[].workload' target/bench-report-fixed.json         # all fixed_*
jq '.results[].workload' target/bench-report-market_state.json  # all market_state_*
```
