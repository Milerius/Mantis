# mantis-layout

Struct layout and cache-line analysis for the Mantis SDK.

`std`-only tooling crate.

## Purpose

Reports size, alignment, and cache-line occupancy for hot-path data structures. Used to verify that performance-critical types have the expected memory layout (e.g., head/tail on separate cache lines).

## Usage

```bash
cargo run -p mantis-layout
```

### As a library

```rust
use mantis_layout::inspect;

let info = inspect::<u64>("u64");
println!("{info}");
// Type: u64
//   size:        8 bytes
//   align:       8 bytes
//   cache lines: 1 (64B)
```

## Output

For each inspected type, prints:
- **size** in bytes
- **alignment** in bytes
- **cache lines** occupied (assuming 64-byte lines)
