# mantis-registry

Instrument registry with venue bindings for the Mantis low-latency financial SDK.

Maps venue-specific identifiers to stable internal `InstrumentId` values with O(1) average lookup time.

## Why

Polymarket creates a NEW market (new `condition_id`, new `token_id`) for every time window. But the logical instrument ("BTC 15m Up") is persistent. The registry provides a stable `InstrumentId` that survives market rotation, so downstream engines never see venue ID churn.

```
token_id "72160714677..." (changes every 15 min)
    ↓ registry.by_polymarket_token_id()
InstrumentId(1) = BTC-15m-Up (stable, never changes)
    ↓ registry.meta()
InstrumentMeta<6> { tick_size, lot_size }
    ↓ price_to_ticks(), qty_to_lots()
Ticks(82), Lots(1500)  → HotEvent payload
```

## Architecture

```
CONTROL PATH (startup, scanner)          HOT BOUNDARY (ingestion thread)

register instruments ──┐                 venue_id ──> InstrumentId     O(1)
discover windows ──────┤                 InstrumentId ──> InstrumentMeta  O(1)
bind/promote/unbind ───┘
                                         No strings after this point.
(may allocate, O(n), API calls)          (no alloc, no iteration, no blocking)
```

The registry is read-mostly. Control-path mutations (startup, window rotation) are infrequent. Hot-boundary reads happen ~2,700 times/sec per instrument.

## Usage

```rust
use mantis_registry::*;
use mantis_fixed::FixedI64;
use mantis_types::*;

// At startup: register logical instruments (IDs are stable, never recycled)
let mut registry = InstrumentRegistry::<6>::new();
let meta = InstrumentMeta::new(
    FixedI64::from_raw(10_000),  // tick_size = 0.01
    FixedI64::from_raw(10_000),  // lot_size = 0.01
).unwrap();

let (btc_up, btc_down) = registry.insert_prediction_pair(
    Asset::Btc,
    Timeframe::M15,
    meta,
    Some("BTCUSDT"),  // Binance reference symbol
)?;
// btc_up = InstrumentId(1), btc_down = InstrumentId(2)

// Scanner discovers a new Polymarket window → bind token IDs
registry.bind_polymarket_current(btc_up, PolymarketWindowBinding {
    token_id: "72160714677...".into(),
    market_slug: "btc-updown-15m-1775280600".into(),
    window_start: Timestamp::from_nanos(0),
    window_end: Timestamp::from_nanos(900_000_000_000),
    condition_id: Some("0x3f06...".into()),
})?;

// Hot boundary: ingestion thread resolves venue IDs
let id = registry.by_polymarket_token_id("72160714677...")?;
let meta = registry.meta(id)?;
let ticks = meta.price_to_ticks(price_fixed)?;
// → emit HotEvent with InstrumentId + Ticks + Lots
```

## Polymarket Window Rotation

```
1. Scanner discovers window via Gamma API       (control path)
2. registry.bind_polymarket_next(id, next)      (pre-subscribe)
3. WS subscribes to next window's token_ids     (~60-120s early)
4. Window starts → promote_polymarket_next(id)  (next → current)
5. market_resolved → unbind_polymarket(id)      (clear old tokens)
6. Repeat
```

`InstrumentId` never changes through rotation. Only the reverse index (`token_id → InstrumentId`) is updated.

## Venue Bindings

| Venue | Binding | Behavior |
|---|---|---|
| Binance | `BinanceBinding { symbol }` | Stable — symbol rarely changes |
| Polymarket | `PolymarketBinding { current, next }` | Dynamic — rotates every market window |

## API

### Read path (hot boundary, O(1))

- `registry.by_polymarket_token_id(token_id)` → `Option<InstrumentId>`
- `registry.by_binance_symbol(symbol)` → `Option<InstrumentId>`
- `registry.by_key(key)` → `Option<InstrumentId>`
- `registry.meta(id)` → `Option<&InstrumentMeta<D>>`
- `registry.get(id)` → `Option<&InstrumentRecord<D>>`

### Mutation path (control, infrequent)

- `registry.insert(key, meta, binance, polymarket)` → `InstrumentId`
- `registry.insert_prediction_pair(asset, timeframe, meta, binance_symbol)` → `(up_id, down_id)`
- `registry.bind_polymarket_current(id, binding)`
- `registry.bind_polymarket_next(id, binding)`
- `registry.promote_polymarket_next(id)`
- `registry.unbind_polymarket(id)`

## Safety

No `unsafe` code. `#![deny(unsafe_code)]` on the crate root. 11 unit tests covering the full registration, lookup, binding, promotion, unbinding, and rotation lifecycle.
