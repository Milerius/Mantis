//! Polymarket market WebSocket JSON decoder producing [`HotEvent`] values.

use mantis_events::{BookDeltaPayload, EventFlags, HotEvent, TradePayload, UpdateAction};
use mantis_fixed::FixedI64;
use mantis_registry::InstrumentRegistry;
use mantis_types::{InstrumentId, InstrumentMeta, Lots, SeqNum, Side, SourceId, Ticks, Timestamp};

use crate::market::schema::{PolymarketBookMsg, PolymarketPriceChangeMsg, PolymarketTradeMsg};

/// Decodes Polymarket market WebSocket messages into [`HotEvent`] values.
///
/// Each call to [`decode`](Self::decode) parses a single JSON message and
/// writes one or more [`HotEvent`] values into the output buffer. The decoder
/// maintains a monotonic sequence number that increments on each emitted event.
///
/// Supports three message types:
/// - `"price_change"` -> single [`BookDelta`](mantis_events::EventBody::BookDelta)
/// - `"last_trade_price"` -> single [`Trade`](mantis_events::EventBody::Trade)
/// - `"book"` -> batch of [`BookDelta`](mantis_events::EventBody::BookDelta) (snapshot)
///
/// The `D` const generic matches the decimal precision of the
/// [`InstrumentMeta`] used for tick/lot conversion.
pub struct PolymarketMarketDecoder<'r, const D: u8> {
    seq: u64,
    source_id: SourceId,
    registry: &'r InstrumentRegistry<D>,
}

impl<'r, const D: u8> PolymarketMarketDecoder<'r, D> {
    /// Create a new decoder bound to a registry.
    #[must_use]
    pub const fn new(source_id: SourceId, registry: &'r InstrumentRegistry<D>) -> Self {
        Self {
            seq: 0,
            source_id,
            registry,
        }
    }

    /// Decode a Polymarket WebSocket JSON message into [`HotEvent`] values.
    ///
    /// Writes events into `out` and returns the number written (0 on parse
    /// failure or unrecognised message type).
    ///
    /// The input `buf` is mutably borrowed because `simd-json` requires
    /// in-place parsing.
    pub fn decode(
        &mut self,
        buf: &mut [u8],
        recv_ts: Timestamp,
        out: &mut [HotEvent; 64],
    ) -> usize {
        if buf.is_empty() {
            return 0;
        }

        let Some(msg_type) = peek_type(buf) else {
            return 0;
        };

        match msg_type {
            "price_change" => self.decode_price_change(buf, recv_ts, out),
            "last_trade_price" => self.decode_trade(buf, recv_ts, out),
            "book" => self.decode_book(buf, recv_ts, out),
            _ => 0,
        }
    }

    /// Decode a `"price_change"` message into a single `BookDelta` event.
    fn decode_price_change(
        &mut self,
        buf: &mut [u8],
        recv_ts: Timestamp,
        out: &mut [HotEvent; 64],
    ) -> usize {
        let Ok(msg) = parse_json::<PolymarketPriceChangeMsg<'_>>(buf) else {
            return 0;
        };

        let Some((instrument_id, meta)) = self.resolve(msg.asset_id) else {
            return 0;
        };

        let Some((price, qty)) = parse_price_qty::<D>(msg.price, msg.size, &meta) else {
            return 0;
        };

        let Some(side) = parse_side(msg.side) else {
            return 0;
        };

        self.seq += 1;
        out[0] = HotEvent::book_delta(
            recv_ts,
            SeqNum::from_raw(self.seq),
            instrument_id,
            self.source_id,
            EventFlags::LAST_IN_BATCH,
            BookDeltaPayload {
                price,
                qty,
                side,
                action: UpdateAction::Change,
                depth: 0,
                _pad: [0; 5],
            },
        );

        1
    }

    /// Decode a `"last_trade_price"` message into a single `Trade` event.
    fn decode_trade(
        &mut self,
        buf: &mut [u8],
        recv_ts: Timestamp,
        out: &mut [HotEvent; 64],
    ) -> usize {
        let Ok(msg) = parse_json::<PolymarketTradeMsg<'_>>(buf) else {
            return 0;
        };

        let Some((instrument_id, meta)) = self.resolve(msg.asset_id) else {
            return 0;
        };

        let Some((price, qty)) = parse_price_qty::<D>(msg.price, msg.size, &meta) else {
            return 0;
        };

        let aggressor = msg.side.and_then(parse_side).unwrap_or(Side::Bid);

        self.seq += 1;
        out[0] = HotEvent::trade(
            recv_ts,
            SeqNum::from_raw(self.seq),
            instrument_id,
            self.source_id,
            EventFlags::LAST_IN_BATCH,
            TradePayload {
                price,
                qty,
                aggressor,
                _pad: [0; 7],
            },
        );

        1
    }

    /// Decode a `"book"` snapshot into a batch of `BookDelta` events.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "depth_idx is bounded by output buffer size (64), fits in u8"
    )]
    fn decode_book(
        &mut self,
        buf: &mut [u8],
        recv_ts: Timestamp,
        out: &mut [HotEvent; 64],
    ) -> usize {
        let Ok(msg) = parse_json::<PolymarketBookMsg<'_>>(buf) else {
            return 0;
        };

        let Some((instrument_id, meta)) = self.resolve(msg.asset_id) else {
            return 0;
        };

        if msg.bids.is_empty() && msg.asks.is_empty() {
            return 0;
        }

        let mut count: usize = 0;
        let total_levels = msg.bids.len() + msg.asks.len();
        let mut truncation_warned = false;

        for (depth_idx, level) in msg.bids.iter().enumerate() {
            if count >= 64 {
                if !truncation_warned {
                    tracing::warn!(
                        asset_id = msg.asset_id,
                        total_levels,
                        emitted = 64,
                        "book snapshot truncated at 64 events"
                    );
                    truncation_warned = true;
                }
                break;
            }
            let Some((price, qty)) = parse_price_qty::<D>(level.price, level.size, &meta) else {
                continue;
            };

            self.seq += 1;
            out[count] = HotEvent::book_delta(
                recv_ts,
                SeqNum::from_raw(self.seq),
                instrument_id,
                self.source_id,
                EventFlags::IS_SNAPSHOT,
                BookDeltaPayload {
                    price,
                    qty,
                    side: Side::Bid,
                    action: UpdateAction::New,
                    depth: depth_idx as u8,
                    _pad: [0; 5],
                },
            );
            count += 1;
        }

        for (depth_idx, level) in msg.asks.iter().enumerate() {
            if count >= 64 {
                if !truncation_warned {
                    tracing::warn!(
                        asset_id = msg.asset_id,
                        total_levels,
                        emitted = 64,
                        "book snapshot truncated at 64 events"
                    );
                }
                break;
            }
            let Some((price, qty)) = parse_price_qty::<D>(level.price, level.size, &meta) else {
                continue;
            };

            self.seq += 1;
            out[count] = HotEvent::book_delta(
                recv_ts,
                SeqNum::from_raw(self.seq),
                instrument_id,
                self.source_id,
                EventFlags::IS_SNAPSHOT,
                BookDeltaPayload {
                    price,
                    qty,
                    side: Side::Ask,
                    action: UpdateAction::New,
                    depth: depth_idx as u8,
                    _pad: [0; 5],
                },
            );
            count += 1;
        }

        // Set LAST_IN_BATCH on the final emitted event (not based on raw JSON count).
        if count > 0 {
            out[count - 1].header.flags |= EventFlags::LAST_IN_BATCH;
        }

        count
    }

    /// Resolve a Polymarket `asset_id` to an `(InstrumentId, InstrumentMeta)`.
    fn resolve(&self, asset_id: &str) -> Option<(InstrumentId, InstrumentMeta<D>)> {
        let id = self.registry.by_polymarket_token_id(asset_id)?;
        let meta = self.registry.meta(id)?;
        Some((id, *meta))
    }
}

/// Parse a Polymarket side string to [`Side`].
fn parse_side(s: &str) -> Option<Side> {
    match s {
        "BUY" => Some(Side::Bid),
        "SELL" => Some(Side::Ask),
        _ => None,
    }
}

/// Parse price and size strings into `(Ticks, Lots)` using the instrument metadata.
fn parse_price_qty<const D: u8>(
    price_str: &str,
    size_str: &str,
    meta: &InstrumentMeta<D>,
) -> Option<(Ticks, Lots)> {
    let price_fixed = FixedI64::<D>::parse_decimal_bytes(price_str.as_bytes()).ok()?;
    let qty_fixed = FixedI64::<D>::parse_decimal_bytes(size_str.as_bytes()).ok()?;
    let price = meta.price_to_ticks(price_fixed)?;
    let qty = meta.qty_to_lots(qty_fixed)?;
    Some((price, qty))
}

/// Scan raw JSON bytes for the `"type"` field value without modifying the buffer.
///
/// Returns the type value as a string slice, or `None` if not found.
/// This is a hot-path optimization that avoids double-parse with `simd-json`.
fn peek_type(buf: &[u8]) -> Option<&str> {
    // Search for the byte pattern: "type":"
    let needle = b"\"type\":\"";
    let pos = memchr_find(buf, needle)?;
    let value_start = pos + needle.len();

    // Find the closing quote
    let remaining = buf.get(value_start..)?;
    let end = memchr_byte(b'"', remaining)?;

    // SAFETY: JSON values in the "type" field are always ASCII
    core::str::from_utf8(remaining.get(..end)?).ok()
}

/// Find the position of `needle` in `haystack`.
fn memchr_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    let limit = haystack.len() - needle.len() + 1;
    for i in 0..limit {
        if haystack.get(i..i + needle.len()) == Some(needle) {
            return Some(i);
        }
    }
    None
}

/// Find the position of a single byte in a slice.
fn memchr_byte(byte: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate() {
        if b == byte {
            return Some(i);
        }
    }
    None
}

#[cfg(feature = "sonic-rs")]
fn parse_json<'a, T: serde::Deserialize<'a>>(buf: &'a mut [u8]) -> Result<T, ()> {
    sonic_rs::from_slice(&*buf).map_err(|_| ())
}

#[cfg(all(feature = "simd-json", not(feature = "sonic-rs")))]
fn parse_json<'a, T: serde::Deserialize<'a>>(buf: &'a mut [u8]) -> Result<T, ()> {
    simd_json::from_slice(buf).map_err(|_| ())
}

#[cfg(not(any(feature = "sonic-rs", feature = "simd-json")))]
fn parse_json<'a, T: serde::Deserialize<'a>>(buf: &'a mut [u8]) -> Result<T, ()> {
    serde_json::from_slice(buf).map_err(|_| ())
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test assertions require panic on unexpected variants"
)]
mod tests {
    use super::*;
    use mantis_events::EventBody;
    use mantis_fixed::FixedI64;
    use mantis_registry::{
        Asset, InstrumentKey, OutcomeSide, PolymarketBinding, PolymarketWindowBinding, Timeframe,
    };
    use mantis_types::Timestamp;

    /// Build a test registry with one BTC/M15/Up prediction instrument
    /// bound to Polymarket token `"abc123"`.
    ///
    /// `tick_size` = 0.01, `lot_size` = 1.0 (6 decimal places)
    ///
    /// # Panics
    ///
    /// Never panics -- hard-coded constants are valid.
    #[expect(
        clippy::expect_used,
        reason = "test-only helper with known-valid constants"
    )]
    fn test_registry() -> (InstrumentRegistry<6>, InstrumentId) {
        let mut reg = InstrumentRegistry::<6>::new();
        // tick_size = 0.01 -> raw 10_000 at D=6
        // lot_size  = 1.0  -> raw 1_000_000 at D=6
        let meta = InstrumentMeta::new(
            FixedI64::<6>::from_raw(10_000),
            FixedI64::<6>::from_raw(1_000_000),
        )
        .expect("valid meta");

        let key = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Up);
        let id = reg
            .insert(key, meta, None, Some(PolymarketBinding::default()))
            .expect("insert ok");

        reg.bind_polymarket_current(
            id,
            PolymarketWindowBinding {
                token_id: "abc123".to_owned(),
                market_slug: "btc-15m-up".to_owned(),
                window_start: Timestamp::from_nanos(0),
                window_end: Timestamp::from_nanos(900_000_000_000),
                condition_id: None,
            },
        )
        .expect("bind ok");

        (reg, id)
    }

    fn make_out() -> [HotEvent; 64] {
        [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            mantis_events::HeartbeatPayload { counter: 0 },
        ); 64]
    }

    // --- Test 1: price_change produces BookDelta ---

    #[test]
    fn decode_price_change_produces_book_delta() {
        let (reg, instrument_id) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"BUY"}"#
                .to_vec();
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(1000);

        let n = decoder.decode(&mut buf, recv_ts, &mut out);
        assert_eq!(n, 1);

        let event = out[0];
        assert_eq!(event.header.recv_ts, recv_ts);
        assert_eq!(event.header.instrument_id, instrument_id);
        assert_eq!(event.header.source_id, SourceId::from_raw(10));
        assert!(event.header.flags.contains(EventFlags::LAST_IN_BATCH));

        if let EventBody::BookDelta(p) = event.body {
            // price: 0.53 / 0.01 = 53 ticks
            assert_eq!(p.price, Ticks::from_raw(53));
            // qty: 100.0 / 1.0 = 100 lots
            assert_eq!(p.qty, Lots::from_raw(100));
            assert_eq!(p.side, Side::Bid);
            assert_eq!(p.action, UpdateAction::Change);
        } else {
            panic!("expected BookDelta, got {:?}", event.kind());
        }
    }

    // --- Test 2: book produces batch with LAST_IN_BATCH on final ---

    #[test]
    fn decode_book_produces_batch_with_last_in_batch() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf = br#"{"type":"book","asset_id":"abc123","bids":[{"price":"0.50","size":"10.0"},{"price":"0.49","size":"20.0"}],"asks":[{"price":"0.51","size":"15.0"}]}"#.to_vec();
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(2000);

        let n = decoder.decode(&mut buf, recv_ts, &mut out);
        assert_eq!(n, 3);

        // First two: IS_SNAPSHOT only
        assert!(out[0].header.flags.contains(EventFlags::IS_SNAPSHOT));
        assert!(!out[0].header.flags.contains(EventFlags::LAST_IN_BATCH));

        assert!(out[1].header.flags.contains(EventFlags::IS_SNAPSHOT));
        assert!(!out[1].header.flags.contains(EventFlags::LAST_IN_BATCH));

        // Last: IS_SNAPSHOT | LAST_IN_BATCH
        assert!(out[2].header.flags.contains(EventFlags::IS_SNAPSHOT));
        assert!(out[2].header.flags.contains(EventFlags::LAST_IN_BATCH));

        // Check sides
        if let EventBody::BookDelta(p) = out[0].body {
            assert_eq!(p.side, Side::Bid);
            assert_eq!(p.depth, 0);
            assert_eq!(p.price, Ticks::from_raw(50));
            assert_eq!(p.action, UpdateAction::New);
        } else {
            panic!("expected BookDelta");
        }

        if let EventBody::BookDelta(p) = out[1].body {
            assert_eq!(p.side, Side::Bid);
            assert_eq!(p.depth, 1);
            assert_eq!(p.price, Ticks::from_raw(49));
        } else {
            panic!("expected BookDelta");
        }

        if let EventBody::BookDelta(p) = out[2].body {
            assert_eq!(p.side, Side::Ask);
            assert_eq!(p.depth, 0);
            assert_eq!(p.price, Ticks::from_raw(51));
        } else {
            panic!("expected BookDelta");
        }
    }

    // --- Test 3: trade produces Trade event ---

    #[test]
    fn decode_trade_produces_trade_event() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"last_trade_price","asset_id":"abc123","price":"0.55","size":"50.0","side":"SELL"}"#
                .to_vec();
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(3000);

        let n = decoder.decode(&mut buf, recv_ts, &mut out);
        assert_eq!(n, 1);

        let event = out[0];
        assert!(event.header.flags.contains(EventFlags::LAST_IN_BATCH));

        if let EventBody::Trade(p) = event.body {
            assert_eq!(p.price, Ticks::from_raw(55));
            assert_eq!(p.qty, Lots::from_raw(50));
            assert_eq!(p.aggressor, Side::Ask);
        } else {
            panic!("expected Trade, got {:?}", event.kind());
        }
    }

    // --- Test 4: non-hot types are skipped ---

    #[test]
    fn decode_skips_non_hot_types() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(0);

        for msg_type in &[
            "best_bid_ask",
            "tick_size_change",
            "market_resolved",
            "new_market",
        ] {
            let json = format!(r#"{{"type":"{msg_type}","asset_id":"abc123"}}"#);
            let mut buf = json.into_bytes();
            let n = decoder.decode(&mut buf, recv_ts, &mut out);
            assert_eq!(n, 0, "expected 0 for type {msg_type}");
        }
    }

    // --- Test 5: unknown token returns zero ---

    #[test]
    fn decode_unknown_token_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"price_change","asset_id":"unknown_token","price":"0.53","size":"100.0","side":"BUY"}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    // --- Test 6: malformed input returns zero ---

    #[test]
    fn decode_malformed_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf = b"not json".to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    // --- Test 7: sequence increments across calls ---

    #[test]
    fn seq_increments_across_calls() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(0);

        let mut buf1 =
            br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"BUY"}"#
                .to_vec();
        let n1 = decoder.decode(&mut buf1, recv_ts, &mut out);
        assert_eq!(n1, 1);
        let seq1 = out[0].header.seq;

        let mut buf2 =
            br#"{"type":"price_change","asset_id":"abc123","price":"0.54","size":"200.0","side":"SELL"}"#
                .to_vec();
        let n2 = decoder.decode(&mut buf2, recv_ts, &mut out);
        assert_eq!(n2, 1);
        let seq2 = out[0].header.seq;

        assert!(seq2 > seq1);
    }

    // --- peek_type unit tests ---

    #[test]
    fn peek_type_extracts_value() {
        let buf = br#"{"type":"price_change","asset_id":"abc"}"#;
        assert_eq!(peek_type(buf), Some("price_change"));
    }

    #[test]
    fn peek_type_returns_none_for_missing() {
        let buf = br#"{"asset_id":"abc"}"#;
        assert_eq!(peek_type(buf), None);
    }

    #[test]
    fn peek_type_returns_none_for_empty() {
        assert_eq!(peek_type(b""), None);
    }

    #[test]
    fn decode_empty_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf: Vec<u8> = Vec::new();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_trade_no_side_defaults_to_bid() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"last_trade_price","asset_id":"abc123","price":"0.55","size":"50.0"}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);

        if let EventBody::Trade(p) = out[0].body {
            assert_eq!(p.aggressor, Side::Bid);
        } else {
            panic!("expected Trade, got {:?}", out[0].kind());
        }
    }

    #[test]
    fn decode_trade_invalid_side_defaults_to_bid() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"last_trade_price","asset_id":"abc123","price":"0.55","size":"50.0","side":"INVALID"}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);

        if let EventBody::Trade(p) = out[0].body {
            assert_eq!(p.aggressor, Side::Bid);
        } else {
            panic!("expected Trade, got {:?}", out[0].kind());
        }
    }

    #[test]
    fn decode_price_change_invalid_price_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"price_change","asset_id":"abc123","price":"notanum","size":"100.0","side":"BUY"}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_price_change_invalid_size_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"bad","side":"BUY"}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_price_change_invalid_side_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf =
            br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"HOLD"}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_book_empty_levels_returns_zero() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        let mut buf = br#"{"type":"book","asset_id":"abc123","bids":[],"asks":[]}"#.to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_book_skips_invalid_price_level() {
        let (reg, _) = test_registry();
        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);
        // First bid has invalid price, second bid is valid
        let mut buf =
            br#"{"type":"book","asset_id":"abc123","bids":[{"price":"bad","size":"10.0"},{"price":"0.50","size":"20.0"}],"asks":[]}"#
                .to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        // Only the valid level should be emitted
        assert_eq!(n, 1);

        if let EventBody::BookDelta(p) = out[0].body {
            assert_eq!(p.price, Ticks::from_raw(50));
            assert_eq!(p.qty, Lots::from_raw(20));
            assert_eq!(p.side, Side::Bid);
        } else {
            panic!("expected BookDelta, got {:?}", out[0].kind());
        }
    }

    #[test]
    fn peek_type_truncated_returns_none() {
        // JSON with "type":" present but no closing quote for the value
        let truncated = b"{ \"type\":\"abc";
        assert_eq!(peek_type(truncated), None);
    }
}
