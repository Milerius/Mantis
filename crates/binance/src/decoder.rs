//! Binance bookTicker JSON decoder producing [`HotEvent`] values.

use mantis_events::{EventFlags, HotEvent, TopOfBookPayload};
use mantis_fixed::FixedI64;
use mantis_types::{InstrumentId, InstrumentMeta, SeqNum, SourceId, Timestamp};

use crate::schema::BinanceBookTicker;

/// Decodes Binance `bookTicker` WebSocket messages into [`HotEvent`] values.
///
/// Each call to [`decode`](Self::decode) parses a single JSON message and
/// writes at most one [`HotEvent`] into the output buffer. The decoder
/// maintains a monotonic sequence number that increments on each successful
/// decode.
///
/// The `D` const generic matches the decimal precision of the
/// [`InstrumentMeta`] used for tick/lot conversion.
pub struct BinanceDecoder<const D: u8> {
    seq: u64,
    source_id: SourceId,
    instrument_id: InstrumentId,
    meta: InstrumentMeta<D>,
}

impl<const D: u8> BinanceDecoder<D> {
    /// Create a new decoder for a specific instrument.
    #[must_use]
    pub const fn new(
        source_id: SourceId,
        instrument_id: InstrumentId,
        meta: InstrumentMeta<D>,
    ) -> Self {
        Self {
            seq: 0,
            source_id,
            instrument_id,
            meta,
        }
    }

    /// Decode a `bookTicker` JSON message into a `HotEvent::TopOfBook`.
    ///
    /// Writes at most one event into `out[0]`. Returns the number of events
    /// written (0 on parse failure, 1 on success).
    ///
    /// The input `buf` is mutably borrowed because `simd-json` requires
    /// in-place parsing.
    pub fn decode(
        &mut self,
        buf: &mut [u8],
        recv_ts: Timestamp,
        out: &mut [HotEvent; 64],
    ) -> usize {
        let Ok(ticker) = parse_json::<BinanceBookTicker<'_>>(buf) else {
            return 0;
        };

        let Ok(bid_price_fixed) = FixedI64::<D>::parse_decimal_bytes(ticker.b.as_bytes()) else {
            return 0;
        };
        let Ok(ask_price_fixed) = FixedI64::<D>::parse_decimal_bytes(ticker.a.as_bytes()) else {
            return 0;
        };
        let Ok(bid_qty_fixed) = FixedI64::<D>::parse_decimal_bytes(ticker.bid_qty.as_bytes())
        else {
            return 0;
        };
        let Ok(ask_qty_fixed) = FixedI64::<D>::parse_decimal_bytes(ticker.ask_qty.as_bytes())
        else {
            return 0;
        };

        let Some(bid_price) = self.meta.price_to_ticks(bid_price_fixed) else {
            return 0;
        };
        let Some(ask_price) = self.meta.price_to_ticks(ask_price_fixed) else {
            return 0;
        };
        let Some(bid_qty) = self.meta.qty_to_lots(bid_qty_fixed) else {
            return 0;
        };
        let Some(ask_qty) = self.meta.qty_to_lots(ask_qty_fixed) else {
            return 0;
        };

        let payload = TopOfBookPayload {
            bid_price,
            bid_qty,
            ask_price,
            ask_qty,
        };

        self.seq += 1;
        out[0] = HotEvent::top_of_book(
            recv_ts,
            SeqNum::from_raw(self.seq),
            self.instrument_id,
            self.source_id,
            EventFlags::EMPTY,
            payload,
        );

        1
    }
}

#[cfg(feature = "simd-json")]
fn parse_json<'a, T: serde::Deserialize<'a>>(buf: &'a mut [u8]) -> Result<T, ()> {
    simd_json::from_slice(buf).map_err(|_| ())
}

#[cfg(not(feature = "simd-json"))]
fn parse_json<'a, T: serde::Deserialize<'a>>(buf: &'a mut [u8]) -> Result<T, ()> {
    serde_json::from_slice(buf).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mantis_events::EventBody;
    use mantis_types::{InstrumentId, Lots, SourceId, Ticks, Timestamp};

    /// Construct a test decoder with D=3.
    ///
    /// # Panics
    ///
    /// Never panics — the hard-coded constants are valid.
    #[expect(
        clippy::expect_used,
        reason = "test-only helper with known-valid constants"
    )]
    fn test_decoder() -> BinanceDecoder<3> {
        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
            FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
        )
        .expect("valid meta");
        BinanceDecoder::new(SourceId::from_raw(2), InstrumentId::from_raw(1), meta)
    }

    fn book_ticker_json() -> Vec<u8> {
        br#"{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1775281508123,"E":1775281508123}"#.to_vec()
    }

    #[test]
    fn decode_book_ticker_produces_top_of_book() {
        let mut decoder = test_decoder();
        let mut buf = book_ticker_json();
        let mut out = [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            mantis_events::HeartbeatPayload { counter: 0 },
        ); 64];
        let recv_ts = Timestamp::from_nanos(999);

        let n = decoder.decode(&mut buf, recv_ts, &mut out);
        assert_eq!(n, 1);

        let event = out[0];
        assert_eq!(event.header.recv_ts, recv_ts);
        assert_eq!(event.header.instrument_id, InstrumentId::from_raw(1));
        assert_eq!(event.header.source_id, SourceId::from_raw(2));
        assert_eq!(event.header.seq, SeqNum::from_raw(1));

        if let EventBody::TopOfBook(p) = event.body {
            // bid_price: 67396.70 / 0.01 = 6739670 ticks
            assert_eq!(p.bid_price, Ticks::from_raw(6_739_670));
            // bid_qty: 8.819 / 0.001 = 8819 lots
            assert_eq!(p.bid_qty, Lots::from_raw(8819));
            // ask_price: 67396.90 / 0.01 = 6739690 ticks
            assert_eq!(p.ask_price, Ticks::from_raw(6_739_690));
            // ask_qty: 7.181 / 0.001 = 7181 lots
            assert_eq!(p.ask_qty, Lots::from_raw(7181));
        } else {
            assert_eq!(out[0].kind(), mantis_events::EventKind::TopOfBook);
        }
    }

    #[test]
    fn decode_increments_seq() {
        let mut decoder = test_decoder();
        let mut out = [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            mantis_events::HeartbeatPayload { counter: 0 },
        ); 64];
        let recv_ts = Timestamp::from_nanos(0);

        let mut buf1 = book_ticker_json();
        let n1 = decoder.decode(&mut buf1, recv_ts, &mut out);
        assert_eq!(n1, 1);
        let seq1 = out[0].header.seq;

        let mut buf2 = book_ticker_json();
        let n2 = decoder.decode(&mut buf2, recv_ts, &mut out);
        assert_eq!(n2, 1);
        let seq2 = out[0].header.seq;

        assert!(seq2 > seq1);
    }

    #[test]
    fn decode_malformed_returns_zero() {
        let mut decoder = test_decoder();
        let mut buf = b"not json".to_vec();
        let mut out = [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            mantis_events::HeartbeatPayload { counter: 0 },
        ); 64];

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_empty_returns_zero() {
        let mut decoder = test_decoder();
        let mut buf: Vec<u8> = Vec::new();
        let mut out = [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            mantis_events::HeartbeatPayload { counter: 0 },
        ); 64];

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }
}
