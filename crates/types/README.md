# mantis-types

Core newtypes, domain types, and semantic wrappers for the Mantis SDK.

`no_std` by default.

## Types

### Queue Primitives

| Type | Purpose |
|---|---|
| `SeqNum(u64)` | Per-queue monotonic sequence number |
| `SlotIndex(usize)` | Index into a ring buffer slot array |
| `PushError<T>` | Error preserving the value that failed to push |
| `QueueError` | `Full` / `Empty` error enum |
| `AssertPowerOfTwo<N>` | Compile-time power-of-2 validation |

### Domain Types

| Type | Purpose |
|---|---|
| `Side` | `Bid` / `Ask` enum with `opposite()` |
| `Timestamp(u64)` | Nanosecond-precision epoch timestamp |
| `OrderId(u64)` | Order identifier |
| `InstrumentId(u32)` | Instrument identifier (`NONE = 0` sentinel reserved) |
| `SourceId(u16)` | Feed/source identifier |

### Hot-Path Types (integer domain)

| Type | Purpose |
|---|---|
| `Ticks(i64)` | Price in venue-specific tick units |
| `Lots(i64)` | Quantity in venue-specific lot units |

These are raw integer newtypes for engine-internal use. Tick/lot size is stored in venue metadata, not in the type.

### Semantic Wrappers (fixed-point domain)

| Type | Inner | Purpose |
|---|---|---|
| `UsdcAmount` | `FixedI64<6>` | USDC-denominated amount |
| `Probability` | `FixedI64<6>` | Probability in [0, 1], range-validated |
| `BtcQty` | `FixedI64<8>` | BTC quantity (satoshi-scale) |

Each wrapper exposes only domain-valid arithmetic. `UsdcAmount * UsdcAmount` is not allowed (dollars times dollars is meaningless). `Probability` validates range on construction and provides `complement()`.

### Conversion Layer

| Type | Purpose |
|---|---|
| `InstrumentMeta<D>` | Tick/lot size, fixed-point to tick/lot conversion |

## Usage

```rust
use mantis_types::{Side, Ticks, Lots, Probability, UsdcAmount, FixedI64};

// Domain types
let side = Side::Bid;
assert_eq!(side.opposite(), Side::Ask);

// Hot-path integer arithmetic
let price = Ticks::from_raw(150);
let doubled = price * 2;

// Semantic wrappers
let p = Probability::from_raw(300_000).unwrap(); // 0.3
let complement = p.complement(); // 0.7

// USDC math
let amount = UsdcAmount::from_raw(1_500_000); // 1.50 USDC
let tripled = amount.checked_mul_int(3).unwrap();
```

### Instrument Metadata Conversion

```rust
use mantis_types::{InstrumentMeta, Ticks, FixedI64};

let meta = InstrumentMeta::new(
    FixedI64::<6>::from_raw(10_000),  // tick_size = 0.01
    FixedI64::<6>::from_raw(1_000),   // lot_size = 0.001
).unwrap();

let price = FixedI64::<6>::from_raw(1_500_000); // 1.50
let ticks = meta.price_to_ticks(price).unwrap(); // 150 ticks
let back = meta.ticks_to_price(ticks).unwrap();  // 1.500000
```
