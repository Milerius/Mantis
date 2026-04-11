//! Binance multi-symbol bookTicker JSON decoder producing [`HotEvent`] values.

use mantis_events::{EventFlags, HotEvent, TopOfBookPayload};
use mantis_fixed::FixedI64;
use mantis_types::{InstrumentId, InstrumentMeta, SeqNum, SourceId, Timestamp};

use crate::schema::{BinanceBookTicker, BinanceCombinedStream};

/// Maximum number of symbols a single [`BinanceDecoder`] can track.
pub const MAX_BINANCE_SYMBOLS: usize = 8;

/// Maximum length of an inline symbol name (bytes).
const MAX_SYMBOL_LEN: usize = 16;

/// A symbol-to-instrument mapping supplied at construction time.
pub struct BinanceSymbolMapping<'a, const D: u8> {
    /// Symbol string (e.g. `"BTCUSDT"`). Bytes are copied into the decoder.
    pub symbol: &'a str,
    /// The instrument ID to emit for this symbol.
    pub instrument_id: InstrumentId,
    /// Tick/lot conversion metadata.
    pub meta: InstrumentMeta<D>,
}

/// Errors returned by [`BinanceDecoder::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderError {
    /// More than [`MAX_BINANCE_SYMBOLS`] mappings were provided.
    TooManySymbols,
    /// Zero mappings were provided.
    EmptyMappings,
    /// A symbol name exceeds [`MAX_SYMBOL_LEN`] bytes.
    SymbolTooLong,
}

impl core::fmt::Display for DecoderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooManySymbols => write!(f, "too many symbols (max {MAX_BINANCE_SYMBOLS})"),
            Self::EmptyMappings => write!(f, "at least one symbol mapping is required"),
            Self::SymbolTooLong => write!(f, "symbol exceeds {MAX_SYMBOL_LEN} bytes"),
        }
    }
}

/// Decodes Binance `bookTicker` WebSocket messages into [`HotEvent`] values.
///
/// Supports both single-stream (`bookTicker` JSON) and combined-stream
/// (`{"stream":"...","data":{...}}`) message formats. On each successful
/// decode the internal sequence number increments monotonically.
///
/// The `D` const generic matches the decimal precision of the
/// [`InstrumentMeta`] used for tick/lot conversion.
pub struct BinanceDecoder<const D: u8> {
    seq: u64,
    source_id: SourceId,
    instrument_ids: [InstrumentId; MAX_BINANCE_SYMBOLS],
    metas: [InstrumentMeta<D>; MAX_BINANCE_SYMBOLS],
    symbol_names: [[u8; MAX_SYMBOL_LEN]; MAX_BINANCE_SYMBOLS],
    symbol_lens: [u8; MAX_BINANCE_SYMBOLS],
    len: usize,
    /// When `true`, messages use the combined-stream wrapper
    /// (`{"stream":"...","data":{...}}`). Set automatically based on the
    /// number of symbol mappings (>1 = combined). Override with
    /// [`set_combined_stream`](Self::set_combined_stream).
    combined_stream: bool,
}

impl<const D: u8> BinanceDecoder<D> {
    /// Create a new multi-symbol decoder.
    ///
    /// # Errors
    ///
    /// Returns [`DecoderError::EmptyMappings`] when `mappings` is empty,
    /// or [`DecoderError::TooManySymbols`] when it exceeds [`MAX_BINANCE_SYMBOLS`].
    pub fn new(
        source_id: SourceId,
        mappings: &[BinanceSymbolMapping<'_, D>],
    ) -> Result<Self, DecoderError> {
        if mappings.is_empty() {
            return Err(DecoderError::EmptyMappings);
        }
        if mappings.len() > MAX_BINANCE_SYMBOLS {
            return Err(DecoderError::TooManySymbols);
        }

        // Fill all slots with the first mapping's meta as a safe default,
        // then overwrite used slots below.
        let mut instrument_ids = [InstrumentId::NONE; MAX_BINANCE_SYMBOLS];
        let mut metas = [mappings[0].meta; MAX_BINANCE_SYMBOLS];
        let mut symbol_names = [[0u8; MAX_SYMBOL_LEN]; MAX_BINANCE_SYMBOLS];
        let mut symbol_lens = [0u8; MAX_BINANCE_SYMBOLS];

        for (i, m) in mappings.iter().enumerate() {
            instrument_ids[i] = m.instrument_id;
            metas[i] = m.meta;
            let sym = m.symbol.as_bytes();
            if sym.len() > MAX_SYMBOL_LEN {
                return Err(DecoderError::SymbolTooLong);
            }
            symbol_names[i][..sym.len()].copy_from_slice(sym);
            // MAX_SYMBOL_LEN is 16, always fits in u8.
            #[expect(clippy::cast_possible_truncation, reason = "MAX_SYMBOL_LEN <= 255")]
            {
                symbol_lens[i] = sym.len() as u8;
            }
        }

        Ok(Self {
            seq: 0,
            source_id,
            instrument_ids,
            metas,
            symbol_names,
            symbol_lens,
            len: mappings.len(),
            combined_stream: mappings.len() > 1,
        })
    }

    /// Override the combined-stream detection.
    ///
    /// By default, `combined_stream` is `true` when more than one symbol
    /// mapping is provided. Call this to force a specific mode (e.g. when a
    /// single-symbol connection still uses the `/stream?streams=` URL).
    pub fn set_combined_stream(&mut self, combined: bool) {
        self.combined_stream = combined;
    }

    /// Find the index of `symbol` in the mapping table, or `None`.
    fn find_symbol(&self, symbol: &[u8]) -> Option<usize> {
        for i in 0..self.len {
            let slen = self.symbol_lens[i] as usize;
            if slen == symbol.len() && self.symbol_names[i][..slen] == *symbol {
                return Some(i);
            }
        }
        None
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
        let ticker = if self.combined_stream {
            let Ok(combined) = parse_json::<BinanceCombinedStream<'_>>(buf) else {
                return 0;
            };
            combined.data
        } else {
            let Ok(t) = parse_json::<BinanceBookTicker<'_>>(buf) else {
                return 0;
            };
            t
        };

        let Some(idx) = self.find_symbol(ticker.s.as_bytes()) else {
            return 0;
        };

        let meta = &self.metas[idx];

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

        let Some(bid_price) = meta.price_to_ticks(bid_price_fixed) else {
            return 0;
        };
        let Some(ask_price) = meta.price_to_ticks(ask_price_fixed) else {
            return 0;
        };
        let Some(bid_qty) = meta.qty_to_lots(bid_qty_fixed) else {
            return 0;
        };
        let Some(ask_qty) = meta.qty_to_lots(ask_qty_fixed) else {
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
            self.instrument_ids[idx],
            self.source_id,
            EventFlags::EMPTY,
            payload,
        );

        1
    }
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
mod tests {
    use super::*;
    use mantis_events::EventBody;
    use mantis_types::{InstrumentId, Lots, SourceId, Ticks, Timestamp};

    fn make_out() -> [HotEvent; 64] {
        [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            mantis_events::HeartbeatPayload { counter: 0 },
        ); 64]
    }

    /// Construct a single-symbol test decoder with D=3 for BTCUSDT.
    ///
    /// # Panics
    ///
    /// Never panics -- the hard-coded constants are valid.
    #[expect(clippy::expect_used, reason = "test-only helper")]
    fn test_decoder() -> BinanceDecoder<3> {
        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
            FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
        )
        .expect("valid meta");
        BinanceDecoder::new(
            SourceId::from_raw(2),
            &[BinanceSymbolMapping {
                symbol: "BTCUSDT",
                instrument_id: InstrumentId::from_raw(1),
                meta,
            }],
        )
        .expect("valid decoder")
    }

    /// Construct a two-symbol test decoder: BTCUSDT (id=1) + ETHUSDT (id=2).
    ///
    /// # Panics
    ///
    /// Never panics -- the hard-coded constants are valid.
    #[expect(clippy::expect_used, reason = "test-only helper")]
    fn multi_decoder() -> BinanceDecoder<3> {
        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
            FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
        )
        .expect("valid meta");
        BinanceDecoder::new(
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
        )
        .expect("valid decoder")
    }

    fn btc_ticker_json() -> Vec<u8> {
        br#"{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1775281508123,"E":1775281508123}"#.to_vec()
    }

    #[test]
    fn decode_single_symbol() {
        let mut decoder = test_decoder();
        let mut buf = btc_ticker_json();
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(999);

        let n = decoder.decode(&mut buf, recv_ts, &mut out);
        assert_eq!(n, 1);

        let event = out[0];
        assert_eq!(event.header.recv_ts, recv_ts);
        assert_eq!(event.header.instrument_id, InstrumentId::from_raw(1));
        assert_eq!(event.header.source_id, SourceId::from_raw(2));
        assert_eq!(event.header.seq, SeqNum::from_raw(1));

        if let EventBody::TopOfBook(p) = event.body {
            assert_eq!(p.bid_price, Ticks::from_raw(6_739_670));
            assert_eq!(p.bid_qty, Lots::from_raw(8819));
            assert_eq!(p.ask_price, Ticks::from_raw(6_739_690));
            assert_eq!(p.ask_qty, Lots::from_raw(7181));
        } else {
            assert_eq!(out[0].kind(), mantis_events::EventKind::TopOfBook);
        }
    }

    fn btc_combined_json() -> Vec<u8> {
        br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1775281508123,"E":1775281508123}}"#.to_vec()
    }

    fn eth_combined_json() -> Vec<u8> {
        br#"{"stream":"ethusdt@bookTicker","data":{"e":"bookTicker","s":"ETHUSDT","b":"3456.70","B":"10.500","a":"3456.90","A":"5.200","T":1775281508123,"E":1775281508123}}"#.to_vec()
    }

    #[test]
    fn decode_multi_symbol_btc() {
        let mut decoder = multi_decoder();
        let mut buf = btc_combined_json();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].header.instrument_id, InstrumentId::from_raw(1));
    }

    #[test]
    fn decode_multi_symbol_eth() {
        let mut decoder = multi_decoder();
        let mut buf = eth_combined_json();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].header.instrument_id, InstrumentId::from_raw(2));
    }

    #[test]
    fn decode_unknown_symbol_returns_zero() {
        let mut decoder = test_decoder();
        let mut buf =
            br#"{"e":"bookTicker","s":"SOLUSDT","b":"100.00","B":"1.000","a":"100.10","A":"2.000","T":1775281508123,"E":1775281508123}"#.to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_combined_stream_wrapper() {
        let mut decoder = test_decoder();
        decoder.set_combined_stream(true);
        let mut buf =
            br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1775281508123,"E":1775281508123}}"#.to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].header.instrument_id, InstrumentId::from_raw(1));

        if let EventBody::TopOfBook(p) = out[0].body {
            assert_eq!(p.bid_price, Ticks::from_raw(6_739_670));
        } else {
            assert_eq!(out[0].kind(), mantis_events::EventKind::TopOfBook);
        }
    }

    #[test]
    fn decode_increments_seq() {
        let mut decoder = test_decoder();
        let mut out = make_out();
        let recv_ts = Timestamp::from_nanos(0);

        let mut buf1 = btc_ticker_json();
        let n1 = decoder.decode(&mut buf1, recv_ts, &mut out);
        assert_eq!(n1, 1);
        let seq1 = out[0].header.seq;

        let mut buf2 = btc_ticker_json();
        let n2 = decoder.decode(&mut buf2, recv_ts, &mut out);
        assert_eq!(n2, 1);
        let seq2 = out[0].header.seq;

        assert!(seq2 > seq1);
    }

    #[test]
    fn decode_malformed_returns_zero() {
        let mut decoder = test_decoder();
        let mut buf = b"not json".to_vec();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    fn decode_empty_returns_zero() {
        let mut decoder = test_decoder();
        let mut buf: Vec<u8> = Vec::new();
        let mut out = make_out();

        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0);
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test-only helper")]
    fn too_many_symbols_errors() {
        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
            FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
        )
        .expect("valid meta");
        let mappings: Vec<BinanceSymbolMapping<'_, 3>> = (0..9)
            .map(|i| BinanceSymbolMapping {
                symbol: "SYM",
                instrument_id: InstrumentId::from_raw(i),
                meta,
            })
            .collect();
        let result = BinanceDecoder::new(SourceId::from_raw(1), &mappings);
        assert_eq!(result.err(), Some(DecoderError::TooManySymbols));
    }

    #[test]
    fn empty_mappings_errors() {
        let mappings: &[BinanceSymbolMapping<'_, 3>] = &[];
        let result = BinanceDecoder::new(SourceId::from_raw(1), mappings);
        assert_eq!(result.err(), Some(DecoderError::EmptyMappings));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test-only helper")]
    fn symbol_too_long_errors() {
        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
            FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
        )
        .expect("valid meta");
        let result = BinanceDecoder::new(
            SourceId::from_raw(1),
            &[BinanceSymbolMapping {
                symbol: "ABCDEFGHIJKLMNOPQ", // 17 bytes > MAX_SYMBOL_LEN (16)
                instrument_id: InstrumentId::from_raw(1),
                meta,
            }],
        );
        assert_eq!(result.err(), Some(DecoderError::SymbolTooLong));
    }
}
