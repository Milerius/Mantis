# mantis-polymarket

Zero-allocation Polymarket CLOB WebSocket decoder for the Mantis low-latency financial SDK.

Converts market WebSocket messages into `HotEvent` values at ~306ns per `price_change` message.

## Architecture

```
COLD SIDE (transport thread)                  HOT SIDE (SPSC consumer)
                                            |
WS frame -> PolymarketMarketDecoder::decode |  -> HotEvent -> SPSC -> engine
  peek_type() → dispatch by message type    |
  simd-json parse → FixedI64               |
  registry.by_polymarket_token_id()        |  O(1) token_id → InstrumentId
  meta.price_to_ticks/qty_to_lots          |
  emit BookDelta / Trade                   |
                                            |
              push()                        |    pop_batch()
```

The decoder borrows an `InstrumentRegistry` for O(1) `token_id` to `InstrumentId` resolution. This supports Polymarket's market rotation model where token IDs change every window but `InstrumentId` remains stable.

## Message Types

| WS `type` | Output | Flags | Description |
|---|---|---|---|
| `price_change` | Single `BookDelta` | `LAST_IN_BATCH` | Level update (price + size + side) |
| `last_trade_price` | Single `Trade` | `LAST_IN_BATCH` | Executed trade with optional aggressor side |
| `book` | Batch of `BookDelta` | `IS_SNAPSHOT`, last gets `LAST_IN_BATCH` | Full book snapshot (bids + asks) |

### Snapshot Truncation

Book snapshots are capped at 64 levels (the output buffer size). If a snapshot exceeds 64 combined bid+ask levels, the decoder emits 64 events and logs a `tracing::warn`. This is a known limitation for deep books.

## Usage

```rust
use mantis_polymarket::market::*;
use mantis_registry::InstrumentRegistry;
use mantis_types::*;

// Registry must outlive the decoder (typically 'static via Box::leak)
let registry: &'static InstrumentRegistry<6> = /* ... */;

let mut decoder = PolymarketMarketDecoder::<6>::new(
    SourceId::from_raw(10),
    registry,
);

// Decode a raw WebSocket message
let mut buf = br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"BUY"}"#.to_vec();
let mut out = [HotEvent::heartbeat(/* ... */); 64];
let n = decoder.decode(&mut buf, Timestamp::now(), &mut out);
// n=1, out[0] = BookDelta { price: Ticks(53), qty: Lots(100), side: Bid }
```

## Type Dispatch

`peek_type()` scans raw bytes for `"type":"` and extracts the value without parsing the full JSON. This allows `simd-json`-safe dispatch: the buffer is only parsed once after the message type is known.

## Spawning a Feed

```rust
use mantis_polymarket::market::*;
use mantis_transport::polymarket::market::PolymarketMarketConfig;

let result = spawn_polymarket_market_feed(
    PolymarketMarketConfig {
        token_ids: vec![up_token.into(), down_token.into()],
        core_id: Some(1),
        backoff: Default::default(),
    },
    SourceId::from_raw(10),
    registry,
    |event| ring.try_push(event).is_ok(),
)?;

// Monitor backpressure
let drops = result.drop_count.load(std::sync::atomic::Ordering::Relaxed);
```

## Key Types

| Type | Description |
|---|---|
| `PolymarketMarketDecoder<'r, D>` | Stateful decoder borrowing an `InstrumentRegistry` |
| `FeedSpawnResult` | Handle + `event_count` + `drop_count` atomic counters |

## Future

- User WebSocket decoder (fills, order acks)
- CLOB REST client (order submission)
- Gamma API client (market discovery, window rotation)
- `TokenIndex` for O(1) token_id lookup without string hashing

## Testing

```bash
cargo +nightly test -p mantis-polymarket
```

## Safety

`#![deny(unsafe_code)]` on the crate root. No unsafe blocks.
