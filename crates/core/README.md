# mantis-core

Strategy traits and default implementations for the Mantis SDK.

`no_std` by default.

## Purpose

Defines the variation points that parameterize all Mantis data structures:

- **`IndexStrategy`** — how indices wrap around buffer capacity
- **`PushPolicy`** — what to do when the queue is full
- **`Instrumentation`** — measurement hooks for push/pop operations

## Provided Implementations

| Type | Trait | Behavior |
|---|---|---|
| `Pow2Masked` | `IndexStrategy` | Bitwise AND mask (requires power-of-2 capacity) |
| `ImmediatePush` | `PushPolicy` | Returns `Err(Full)` immediately |
| `NoInstr` | `Instrumentation` | No-op, zero overhead |
| `CountingInstr` | `Instrumentation` | Atomic counters for push/pop/full/empty |

## Usage

```rust
use mantis_core::{IndexStrategy, Pow2Masked};

let wrapped = Pow2Masked::wrap(1025, 1024); // -> 1
```

Typically used indirectly through `mantis-queue` preset types.
