# mantis-fixed

Compile-time-scaled fixed-point decimal arithmetic for the Mantis SDK.

`no_std` by default. Zero unsafe code. Faster than `rust_decimal` and the `fixed` crate on multiply.

## Architecture

```
                  mantis-fixed                    mantis-types
               (numeric engine)               (domain semantics)

           FixedI64<const D: u8>         UsdcAmount(FixedI64<6>)
                  |                      Probability(FixedI64<6>)
     checked_mul_trunc / _round          BtcQty(FixedI64<8>)
     checked_div_trunc / _round          Ticks(i64)  Lots(i64)
     rescale_trunc / _round / _exact     InstrumentMeta<D>
     from_str_decimal / Display
```

### Multi-Tier Numeric Model

| Layer | Types | Domain |
|-------|-------|--------|
| Hot path (engine) | `Ticks(i64)`, `Lots(i64)` | Order book, matching, signals |
| Boundary (normalization) | `UsdcAmount`, `Probability`, `BtcQty` | Parsing, risk, display |
| Instrument metadata | `InstrumentMeta<D>` | Tick/lot size, conversion |

## Usage

```rust
use mantis_fixed::FixedI64;

// Create values at 6 decimal places (USDC-scale)
let price = FixedI64::<6>::from_str_decimal("1.500000").unwrap();
let qty = FixedI64::<6>::from_int(3).unwrap();

// Multiply (explicit rounding policy)
let notional = price.checked_mul_int(qty.to_raw()).unwrap();
assert_eq!(format!("{notional}"), "4.500000");

// Scale conversion
let price_d2: FixedI64<2> = price.rescale_trunc().unwrap();
assert_eq!(format!("{price_d2}"), "1.50");
```

### Overflow Handling

```rust
use mantis_fixed::FixedI64;

type F6 = FixedI64<6>;

let a = F6::from_raw(1_500_000); // 1.5
let b = F6::from_raw(2_000_000); // 2.0

// Checked (returns None on overflow)
let product = a.checked_mul_trunc(b); // Some(3.000000)

// Saturating (clamps to MIN/MAX)
let clamped = F6::MAX.saturating_mul_trunc(b); // F6::MAX

// Operators (debug-panic, release-wrap like Rust integers)
let sum = a + b; // 3.500000
```

## Validated Scales

| D | Use case | Max whole value |
|---|----------|-----------------|
| 2 | Cents, bps, Polymarket display | ~92 quadrillion |
| 4 | Sub-cent precision, CeFi grids | ~922 trillion |
| 6 | USDC, stablecoin math | ~9.2 trillion |
| 8 | BTC quantities (satoshi-scale) | ~92 billion |

D=2, 4, 6, 8 are tested, benchmarked, and documented as production-ready. Other values up to D=18 compile but are not part of the validated set.

## API

### Arithmetic

| Method | Rounding | Returns |
|--------|----------|---------|
| `checked_mul_trunc` | Toward zero | `Option<Self>` |
| `checked_mul_round` | Ties away from zero | `Option<Self>` |
| `checked_div_trunc` | Toward zero | `Option<Self>` |
| `checked_div_round` | Ties away from zero | `Option<Self>` |
| `checked_mul_int` | Exact (integer scale) | `Option<Self>` |
| `checked_div_int` | Toward zero | `Option<Self>` |
| `checked_add` / `checked_sub` | N/A | `Option<Self>` |
| `saturating_*` / `wrapping_*` | Same as checked | `Self` |

No `Mul` / `Div` trait impls. Rounding policy must be explicit.

### Scale Conversion

| Method | Behavior |
|--------|----------|
| `rescale_trunc::<D2>()` | Narrow truncates, widen exact |
| `rescale_round::<D2>()` | Narrow rounds half-up |
| `checked_rescale_exact::<D2>()` | None if narrowing loses digits |

### Parsing / Formatting

| Method | Example |
|--------|---------|
| `from_str_decimal("1.5")` | `Ok(FixedI64<6>(1500000))` |
| `Display` | `"1.500000"` (always D decimal places) |
| `Debug` | `"FixedI64<6>(raw=1500000, value=1.500000)"` |

## Features

| Feature | Default | Effect |
|---------|---------|--------|
| `std` | off | Enables `std::error::Error` on `ParseFixedError` |

## Performance

Measured on Apple M4 Pro (aarch64), `cargo +nightly bench --bench fixed`:

| Operation | Mantis | `fixed` crate | `rust_decimal` | raw `i64` |
|-----------|--------|---------------|----------------|-----------|
| `checked_add` | **499 ps** | 504 ps | 2.18 ns | 497 ps |
| `checked_mul_trunc` | **1.10 ns** | 1.20 ns | 2.32 ns | -- |
| `checked_mul_round` | **1.44 ns** | -- | -- | -- |
| `checked_div_trunc` | **1.94 ns** | 2.17 ns | 3.88 ns | -- |
| `rescale` | **320 ps** | -- | -- | -- |
| `from_str_decimal` | 15.7 ns | 38.4 ns | **3.2 ns** | -- |
| `Display` | 21.1 ns | 20.7 ns | 20.4 ns | -- |

Addition has zero abstraction overhead vs raw `i64`. Multiplication avoids the `__divti3` runtime call via decomposed division that LLVM strength-reduces to multiply-by-reciprocal.

## Safety

No unsafe code. The crate root has `#![deny(unsafe_code)]`. Validated by Miri (110/110 tests pass).
