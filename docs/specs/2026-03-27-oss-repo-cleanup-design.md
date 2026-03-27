# OSS Repo Cleanup — Design Spec

**Date:** 2026-03-27
**Goal:** Prepare the Mantis repository for open source contributions by removing internal development artifacts from git tracking, adding standard OSS files, and integrating branding assets.

## Reference Libraries

Repo structure decisions informed by these top Rust OSS libraries:

| Library | Notable patterns |
|---|---|
| crossbeam | CHANGELOG.md, dual license, `.rustfmt.toml`, no internal docs |
| tokio | CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md, `docs/contributing/` |
| rayon | RELEASES.md, minimal root, no docs folder |
| rtrb | Minimal — README, LICENSE, benches, src, tests |
| parking_lot | CHANGELOG.md, minimal root, benchmark dir |

**Common pattern:** No library ships internal specs, plans, brainstorms, or AI configuration files. Public docs are limited to README, LICENSE, CHANGELOG, and optionally CONTRIBUTING + SECURITY.

## Approach

Single branch with 4 atomic commits. Each commit is independently reviewable and revertable.

## Commit 1: Remove internal dev artifacts from git tracking

**Action:** `git rm --cached -r` (files remain on disk)

Directories removed from tracking:
- `docs/plans/` — internal implementation plans
- `docs/specs/` — internal design specs (including this file, after commit)
- `docs/brainstorms/` — internal brainstorm notes
- `docs/superpowers/` — superpowers plugin output
- `philosophy/` — internal SDK vision documents

Directories that stay tracked:
- `docs/PROGRESS.md` — public project status
- `docs/UNSAFE.md` — public unsafe policy
- `CLAUDE.md` — AI development config
- `.claude/` — AI memory/config

**.gitignore additions:**
```
# Internal development artifacts (kept locally)
docs/plans/
docs/specs/
docs/brainstorms/
docs/superpowers/
philosophy/
```

## Commit 2: Add assets and README banner

**Asset renames:**
- `assets/image.png` → `assets/logo.png` (circular badge)
- `assets/image_2.png` → `assets/banner.png` (wide banner: "Financial Low-Latency HFT Library in Rust")

**README change:**
- Add centered banner image before the `# Mantis` heading
- No other README content changes

## Commit 3: Add CONTRIBUTING.md, CHANGELOG.md, SECURITY.md

### CONTRIBUTING.md

Middle-ground style covering:
- **Prerequisites** — nightly Rust toolchain (`cargo +nightly`)
- **Build / test / lint commands** — extracted from CLAUDE.md quick reference
- **`no_std` rules** — no heap in hot paths, no panics in hot paths
- **Unsafe policy** — isolated in `raw` modules, `// SAFETY:` comments required, Miri on every PR
- **Benchmark protocol** — never claim "fastest", JSON export, contenders behind `bench-contenders` feature flag
- **Commit conventions** — imperative mood, ≤72 char subject, one logical change per commit
- **PR expectations** — feature branches, run fmt + clippy + test before submitting

### CHANGELOG.md

[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format:
- Header with format description
- Empty `[Unreleased]` section
- Ready to populate at first release

### SECURITY.md

GitHub Private Vulnerability Reporting:
- Direct reporters to the repo's Security tab → "Report a vulnerability"
- No personal email exposed

## Commit 4: Update CLAUDE.md references

Remove or update references to directories no longer tracked:
- `philosophy/fin_sdk_oss_blueprint.md` reference in Architecture section
- `philosophy/benchmark_tooling_modular_strategy_design.md` reference in Architecture section
- Any `docs/specs/` references

These files still exist locally, so CLAUDE.md references can point to them as local-only resources or be removed entirely.

## Out of Scope

- MIT dual licensing (staying Apache-2.0 only)
- CODE_OF_CONDUCT.md (not needed at this stage)
- README content changes beyond banner addition
- Restructuring crate layout
- CI changes
