# Unsafe Policy

## Rules

1. All unsafe code lives in `raw` submodules only.
2. Crate roots declare `#![deny(unsafe_code)]`.
3. Only `raw/mod.rs` declares `#![allow(unsafe_code)]`.
4. Every `unsafe` block has a `// SAFETY:` comment that states:
   - Which invariant makes this safe
   - What the caller must guarantee
   - What could go wrong if the invariant is violated
5. Every unsafe function documents preconditions in rustdoc.

## Verification Tiers

| Tier | Method | Runs |
|---|---|---|
| 1 | Unit tests exercising safe API boundaries | Every PR |
| 2 | Miri (stacked borrows + tree borrows) | Every PR |
| 3 | cargo careful (stdlib debug assertions) | Every PR |
| 4 | Kani bounded model checking | Nightly |
| 5 | Differential testing across implementations | Every PR |
| 6 | cargo-mutants (mutation testing) | Nightly |

## Allowed Unsafe Patterns

- `core::sync::atomic` operations with documented ordering rationale
- `MaybeUninit` for uninitialized slot storage in ring buffers
- `core::arch::asm!` for platform-specific fast paths
- `#[repr(C)]` / `#[repr(align)]` casts for layout-controlled types
- `UnsafeCell` for interior mutability in single-writer structures

## Forbidden

- Raw pointer arithmetic when slice indexing works
- `transmute` — use `from_bytes` / `to_bytes` or specific safe casts
- `unsafe impl Send/Sync` without kani proof or formal argument
- Unsafe in tests (use safe API to test unsafe internals)
