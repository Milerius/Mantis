# mantis-types

Core newtypes and error types for the Mantis SDK.

`no_std` by default.

## Types

| Type | Purpose |
|---|---|
| `SeqNum(u64)` | Sequence number for event ordering |
| `SlotIndex(usize)` | Index into a ring buffer slot array |
| `PushError<T>` | Error preserving the value that failed to push |
| `QueueError` | `Full` / `Empty` error enum |
| `AssertPowerOfTwo<N>` | Compile-time power-of-2 validation |

## Usage

```rust
use mantis_types::{SeqNum, PushError};

let seq = SeqNum(42);
let err: PushError<u64> = PushError::Full(100);
match err {
    PushError::Full(val) => assert_eq!(val, 100),
}
```
