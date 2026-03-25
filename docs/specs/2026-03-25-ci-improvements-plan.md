# CI Improvements — Implementation Plan (4 of 4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enhance CI with benchmark regression tracking, ASM feature toggle testing, coverage reporting, test result annotations, and Godbolt ASM nightly job.

**Architecture:** Extend existing GitHub Actions workflows with new jobs and steps. Use `benchmark-action/github-action-benchmark` for regression tracking, `dorny/test-reporter` for test annotations, and codecov for coverage.

**Tech Stack:** GitHub Actions, `benchmark-action/github-action-benchmark`, `dorny/test-reporter`, codecov, Godbolt API.

**Spec:** `docs/specs/2026-03-25-spsc-ring-bench-design.md` — Section 6 (CI Improvements).

**Prerequisite:** Plans 1-3 should be complete so all tests and benchmarks exist.

**CRITICAL:** All GitHub Actions must be pinned to full SHA hashes with version comments per CLAUDE.md: `actions/checkout@<full-sha> # vX.Y.Z`. The implementer MUST look up current SHAs before writing YAML. Use existing `ci.yml` actions as reference for the SHA format.

---

## File Structure

### Modified files

| File | Changes |
|---|---|
| `.github/workflows/ci.yml` | Add ASM toggle steps, nextest + test-reporter, codecov upload |
| `.github/workflows/bench.yml` | Add benchmark-action for regression tracking |
| `.github/workflows/nightly.yml` | Add Godbolt ASM job, fuzz corpus upload, kani reporting |

---

## Task 1: ASM feature toggle in CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Read current ci.yml**

Read `.github/workflows/ci.yml` to understand the current structure. Note:
- Existing SHAs: `actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4`
- All checkouts use `persist-credentials: false`
- Tests run on `ubuntu-latest` + `macos-latest` matrix

- [ ] **Step 2: Add ASM test steps to the `test` job**

Add two steps to the existing `test` job (after the current `cargo test --all-features`):

```yaml
      - name: Test with ASM feature
        run: cargo test --features asm

      - name: Test without ASM (portable fallback)
        run: cargo test --no-default-features -p mantis-queue -p mantis-core -p mantis-types
```

The `asm` feature on x86_64 enables RDTSC; on ARM64 it's a no-op (falls back to `InstantCounter`). Both architectures should test both feature states. The existing `test-no-std` job already covers `--no-default-features` but only for the core crates — this step validates that the `asm` feature compiles and tests pass on all runners.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add ASM feature toggle to test job"
```

---

## Task 2: Test reporter with cargo-nextest

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Look up current SHAs for new actions**

Look up the latest SHA for:
- `taiki-e/install-action` (already in ci.yml: `@328a871ad8f62ecac78390391f463ccabc974b72 # v2`)
- `dorny/test-reporter` — look up latest stable release SHA

- [ ] **Step 2: Add nextest + test-reporter to the `test` job**

Replace the `cargo test --all-features` step with nextest:

```yaml
      - name: Install nextest
        uses: taiki-e/install-action@328a871ad8f62ecac78390391f463ccabc974b72 # v2
        with:
          tool: cargo-nextest

      - name: Run tests
        run: cargo nextest run --all-features --profile ci

      - name: Test Report
        uses: dorny/test-reporter@<LOOK-UP-SHA> # v1
        if: always()
        with:
          name: Rust Tests (${{ matrix.os }})
          path: target/nextest/ci/junit.xml
          reporter: java-junit
```

Note: A `.config/nextest.toml` file must be created with a `ci` profile that outputs JUnit:

```toml
[profile.ci]
default-filter = "all()"

[profile.ci.junit]
path = "junit.xml"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml .config/nextest.toml
git commit -m "ci: add nextest + test-reporter for PR annotations"
```

---

## Task 3: Benchmark regression tracking

**Files:**
- Modify: `.github/workflows/bench.yml`

- [ ] **Step 1: Read current bench.yml**

Read `.github/workflows/bench.yml` to understand the existing structure.

- [ ] **Step 2: Look up SHA for benchmark-action**

Look up the latest SHA for `benchmark-action/github-action-benchmark`.

- [ ] **Step 3: Add benchmark-action step**

Criterion with `tool: cargo` expects Criterion's default bencher-compatible output. Add after the bench run step:

```yaml
      - name: Run benchmarks
        run: cargo bench --all-features 2>&1 | tee bench-output.txt

      - name: Store benchmark result
        uses: benchmark-action/github-action-benchmark@<LOOK-UP-SHA> # v1
        with:
          name: SPSC Ring Benchmarks
          tool: cargo
          output-file-path: bench-output.txt
          github-token: ${{ secrets.GITHUB_TOKEN }}
          auto-push: true
          alert-threshold: "105%"
          comment-on-alert: true
          fail-on-alert: true
          alert-comment-cc-users: "@mantis-maintainers"
```

The spec requires 5% regression threshold with failure. `alert-threshold: "105%"` means "alert if new value > 105% of baseline" (i.e., >5% regression). `fail-on-alert: true` per spec. If CI noise causes false positives on shared runners, this can be relaxed later — but start strict per spec.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/bench.yml
git commit -m "ci: add benchmark regression tracking (5% threshold)"
```

---

## Task 4: Coverage reporting with Codecov

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Look up SHA for codecov-action**

Look up the latest SHA for `codecov/codecov-action`.

- [ ] **Step 2: Add codecov upload to existing coverage job**

Add after the existing "Branch coverage" step, reusing the already-generated `codecov.json`:

```yaml
      - name: Upload to Codecov
        if: github.event_name == 'pull_request'
        uses: codecov/codecov-action@<LOOK-UP-SHA> # v4
        with:
          files: codecov.json
          token: ${{ secrets.CODECOV_TOKEN }}
          fail_ci_if_error: true
```

Note: Uses the existing `codecov.json` (already generated by the "Line coverage" step). No need for a third llvm-cov invocation. `fail_ci_if_error: true` per spec requirement to fail if coverage drops.

- [ ] **Step 3: Add coverage badge to README**

Add to README.md:

```markdown
[![codecov](https://codecov.io/gh/mantis-sdk/mantis/graph/badge.svg)](https://codecov.io/gh/mantis-sdk/mantis)
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml README.md
git commit -m "ci: add codecov coverage reporting"
```

---

## Task 5: Nightly enhancements (Godbolt, fuzz, kani reporting)

**Files:**
- Modify: `.github/workflows/nightly.yml`

- [ ] **Step 1: Read current nightly.yml**

Read `.github/workflows/nightly.yml`.

- [ ] **Step 2: Look up SHAs for all new actions**

Look up current SHAs for:
- `actions/checkout` (use existing: `@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4`)
- `actions/upload-artifact` (use existing: `@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4`)

All `uses:` lines must have full SHA + version comment. All `actions/checkout` steps must include `persist-credentials: false`.

- [ ] **Step 3: Add Godbolt ASM job**

```yaml
  asm-check:
    name: ASM Inspection
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
        with:
          persist-credentials: false

      - name: Install jq
        run: sudo apt-get install -y jq

      - name: Run ASM check
        run: ./scripts/check-asm.sh

      - name: Upload ASM artifacts
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        with:
          name: asm-output
          path: target/asm/
          retention-days: 30
```

- [ ] **Step 4: Add kani proof reporting**

Note: Check `model-checking/kani-github-action` docs for correct invocation. It may require `kani-version` input or a composite action format. The implementer must verify the action interface before writing the YAML.

```yaml
  kani:
    name: Kani Proofs
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
        with:
          persist-credentials: false

      - name: Install Kani
        run: |
          cargo install --locked kani-verifier
          cargo kani setup

      - name: Run proofs
        run: cargo kani -p mantis-verify 2>&1 | tee kani-output.txt

      - name: Kani Summary
        if: always()
        run: |
          echo "## Kani Proof Results" >> $GITHUB_STEP_SUMMARY
          grep -E "(VERIFIED|FAILED|harness)" kani-output.txt >> $GITHUB_STEP_SUMMARY || true

      - name: Upload proof output
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        if: always()
        with:
          name: kani-proofs
          path: kani-output.txt
```

- [ ] **Step 5: Add fuzz corpus persistence**

Add to the existing fuzz job (after fuzz steps):

```yaml
      - name: Upload corpus
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        if: always()
        with:
          name: fuzz-corpus
          path: fuzz/corpus/
          retention-days: 90

      - name: Upload crashes
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4
        if: failure()
        with:
          name: fuzz-crashes
          path: fuzz/artifacts/

      - name: Fuzz Summary
        if: always()
        run: |
          echo "## Fuzz Results" >> $GITHUB_STEP_SUMMARY
          echo "Duration: 10 minutes per target" >> $GITHUB_STEP_SUMMARY
          echo "Corpus size: $(find fuzz/corpus -type f | wc -l) inputs" >> $GITHUB_STEP_SUMMARY
```

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/nightly.yml
git commit -m "ci: enhance nightly with ASM check, kani reporting, fuzz persistence"
```

---

## Task 6: Update `docs/PROGRESS.md`

**Files:**
- Modify: `docs/PROGRESS.md`

- [ ] **Step 1: Update CI-related items**

Mark Phase 1.1 fully complete (all items checked). Update the status line:

```
## Phase 1 — Minimal Useful Core

**Status: In Progress** | Started: 2026-03-25

### 1.1 SPSC Ring Buffer (`mantis-queue`)
**Status: Complete**
```

- [ ] **Step 2: Commit**

```bash
git add docs/PROGRESS.md
git commit -m "docs: update PROGRESS.md with CI improvements"
```

---

## Summary

| Task | What | Commit |
|---|---|---|
| 1 | ASM feature toggle in CI | `ci: ASM feature toggle` |
| 2 | Nextest + test-reporter | `ci: nextest + test-reporter` |
| 3 | Benchmark regression (5% threshold) | `ci: benchmark regression` |
| 4 | Codecov coverage reporting | `ci: codecov` |
| 5 | Nightly enhancements | `ci: nightly enhancements` |
| 6 | Progress doc update | `docs: update PROGRESS.md` |

**Total: 6 tasks, ~6 commits.**
