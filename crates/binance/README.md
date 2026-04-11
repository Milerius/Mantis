# mantis-binance

Zero-allocation Binance WebSocket decoder for the Mantis low-latency financial SDK.

Converts `bookTicker` JSON messages into `HotEvent::TopOfBook` values at ~307ns per message.

## Architecture

```text
COLD SIDE (transport thread)              HOT SIDE (SPSC consumer)
                                        |
WS frame -> BinanceDecoder::decode()    |  -> HotEvent::TopOfBook -> SPSC -> engine
  combined_stream flag → wrapper skip   |
  sonic-rs parse → FixedI64             |
  meta.price_to_ticks/qty_to_lots       |
  emit TopOfBook                        |
                                        |
              push()                    |    pop_batch()
```

The decoder runs on the transport IO thread. It accepts raw `&mut [u8]` buffers, resolves symbols via a flat array lookup, and emits `HotEvent` values through a push callback.

## Multi-Symbol Support

A single `BinanceDecoder` maps 1-8 symbols to `InstrumentId` values via `BinanceSymbolMapping`. Symbol lookup is a flat linear scan over inline `[u8; 16]` names -- no HashMap, no allocation.

```rust
use mantis_binance::*;
use mantis_fixed::FixedI64;
use mantis_types::*;

let meta = InstrumentMeta::new(
    FixedI64::<3>::from_str_decimal("0.01").unwrap(),  // tick_size
    FixedI64::<3>::from_str_decimal("0.001").unwrap(), // lot_size
).unwrap();

let mut decoder = BinanceDecoder::<3>::new(
    SourceId::from_raw(2),
    &[
        BinanceSymbolMapping {
            symbol: "BTCUSDT",
            instrument_id: InstrumentId::from_raw(1),
            meta,
        },
        BinanceSymbolMapping {
            symbol: "ETHUSDT",
            instrument_id: InstrumentId::from_raw(2),
            meta,
        },
    ],
).unwrap();
```

## Combined Stream Detection

Binance sends either plain `bookTicker` JSON or a combined-stream wrapper (`{"stream":"...","data":{...}}`). The `combined_stream` bool field (set automatically when >1 symbol, or overridden via `set_combined_stream`) controls which JSON shape the decoder expects.

## Spawning a Feed

`spawn_binance_feed` wires the decoder to a transport thread with backpressure tracking:

```rust
use mantis_binance::*;
use mantis_transport::binance::reference::BinanceReferenceConfig;

let result = spawn_binance_feed(
    BinanceReferenceConfig::default(),
    decoder,
    |event| ring.try_push(event).is_ok(),
)?;

// Monitor backpressure
let drops = result.drop_count.load(std::sync::atomic::Ordering::Relaxed);
```

## Key Types

| Type | Description |
|---|---|
| `BinanceDecoder<D>` | Stateful decoder with monotonic sequence numbers |
| `BinanceSymbolMapping<'a, D>` | Symbol-to-instrument binding (provided at construction) |
| `FeedSpawnResult` | Handle + `event_count` + `drop_count` atomic counters |
| `DecoderError` | `TooManySymbols`, `EmptyMappings`, or `SymbolTooLong` |

## Features

| Feature | Effect |
|---|---|
| `sonic-rs` (default) | Fast JSON parsing via `sonic-rs` |
| `simd-json` | SIMD-accelerated JSON parsing via `simd-json` (used when `sonic-rs` is not enabled) |
| (neither) | Falls back to `serde_json` |

## Testing

```bash
cargo +nightly test -p mantis-binance
```

## Safety

`#![deny(unsafe_code)]` on the crate root. No unsafe blocks.
