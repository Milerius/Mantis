# mantis-events

Hot event language for the Mantis low-latency financial SDK.

`no_std` by default. `Copy`. `repr(C)`. 64 bytes per event.

## Architecture

```
COLD SIDE (async, alloc OK)              HOT SIDE (no alloc, Ticks/Lots only)
                                       |
WS frame -> parse -> FixedI64 -> Meta  |  -> HotEvent -> SPSC -> engine drain
                                       |
                              push()   |    pop_batch()
```

`mantis-events` depends on `mantis-types` but NOT on `mantis-fixed`. This is a structural dependency firewall: `FixedI64` cannot appear in hot event payloads. Decimal parsing and normalization happen at the ingestion boundary, before events enter the hot path.

## Event Envelope

```
HotEvent (64 bytes, repr(C)):
+-- EventHeader (24 bytes, always at offset 0)
|   +-- recv_ts: Timestamp          8 bytes
|   +-- seq: SeqNum                 8 bytes
|   +-- instrument_id: InstrumentId 4 bytes
|   +-- source_id: SourceId         2 bytes
|   +-- flags: EventFlags           2 bytes
|
+-- EventBody (40 bytes, repr(C, u16))
    +-- discriminant                2 bytes
    +-- alignment padding           6 bytes
    +-- payload                   <=32 bytes
```

Header is always at offset 0 for O(1) access without matching the payload.

## Event Variants

### Market Data

| Variant | Payload | Size | Description |
|---|---|---|---|
| `BookDelta` | `BookDeltaPayload` | 24B | Single price level insert/update/delete |
| `Trade` | `TradePayload` | 24B | Executed trade on venue |
| `TopOfBook` | `TopOfBookPayload` | 32B | Best bid/ask snapshot |

### Execution Lifecycle

| Variant | Payload | Size | Description |
|---|---|---|---|
| `OrderAck` | `OrderAckPayload` | 24B | Order accepted/cancelled/expired |
| `Fill` | `FillPayload` | 32B | Partial or full fill |
| `OrderReject` | `OrderRejectPayload` | 24B | Order rejected by venue |

### Control / System

| Variant | Payload | Size | Description |
|---|---|---|---|
| `Timer` | `TimerPayload` | 8B | Stale-feed, periodic, or deadline timer |
| `Heartbeat` | `HeartbeatPayload` | 4B | Internal liveness ping |

## Usage

```rust
use mantis_events::*;
use mantis_types::*;

// Construct a book delta event
let event = HotEvent::book_delta(
    Timestamp::from_nanos(1_000_000),
    SeqNum::from_raw(1),
    InstrumentId::from_raw(42),
    SourceId::from_raw(1),
    EventFlags::IS_SNAPSHOT,
    BookDeltaPayload {
        price: Ticks::from_raw(5500),
        qty: Lots::from_raw(100),
        side: Side::Bid,
        action: UpdateAction::New,
        depth: 0,
        _pad: [0; 5],
    },
);

// O(1) header access without matching the body
assert_eq!(event.header.instrument_id, InstrumentId::from_raw(42));
assert!(event.header.flags.contains(EventFlags::IS_SNAPSHOT));

// Kind extraction without matching
assert_eq!(event.kind(), EventKind::BookDelta);

// Pattern match the body for full payload access
match &event.body {
    EventBody::BookDelta(delta) => {
        assert_eq!(delta.price, Ticks::from_raw(5500));
    }
    _ => {}
}
```

## EventFlags

Cross-cutting bitflags in the header:

| Flag | Bit | Purpose |
|---|---|---|
| `IS_SNAPSHOT` | 0 | Event is part of a snapshot, not incremental |
| `LAST_IN_BATCH` | 1 | Last event in a batch from a single source message |

## Design Constraints

- **64-byte size ceiling** enforced by `const_assert!(size_of::<HotEvent>() <= 64)`
- **Header at offset 0** verified by layout assertions in `mantis-layout`
- **No `FixedI64` in payloads** enforced by dependency firewall (no `mantis-fixed` dependency)
- **All types are `Copy`** for zero-overhead SPSC queue transport
- **No `Default` or `zeroed()`** on `HotEvent` by design; use `MaybeUninit` for scratch buffers
- **`EventKind` ↔ `EventBody` 1:1 mapping** enforced by exhaustive match with no wildcard

## Features

| Feature | Effect |
|---|---|
| (default) | `no_std`, no allocator |
| `std` | Standard library support (enables `mantis-types/std`) |

## Safety

No `unsafe` code. `#![deny(unsafe_code)]` on the crate root. All layout correctness verified through `repr(C)` + const assertions + `mantis-layout` tests. Miri clean (57/57 tests pass, zero UB).
