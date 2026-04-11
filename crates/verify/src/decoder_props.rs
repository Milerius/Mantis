//! Bolero property tests for venue decoders.
//!
//! Ensures that both the Binance and Polymarket decoders never panic
//! on arbitrary input — critical for HFT where a panic on malformed
//! data crashes the trading system.

#[cfg(test)]
mod tests {
    use bolero::check;
    use mantis_events::{EventFlags, HeartbeatPayload, HotEvent};
    use mantis_fixed::FixedI64;
    use mantis_types::{InstrumentId, InstrumentMeta, SeqNum, SourceId, Timestamp};

    fn make_out() -> [HotEvent; 64] {
        [HotEvent::heartbeat(
            Timestamp::ZERO,
            SeqNum::ZERO,
            SourceId::from_raw(0),
            EventFlags::EMPTY,
            HeartbeatPayload { counter: 0 },
        ); 64]
    }

    // ---- Binance decoder property tests ----

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test-only setup with known-valid constants"
    )]
    fn binance_decoder_never_panics() {
        use mantis_binance::{BinanceDecoder, BinanceSymbolMapping};

        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
            FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
        )
        .expect("valid meta");

        let mut decoder = BinanceDecoder::new(
            SourceId::from_raw(2),
            &[BinanceSymbolMapping {
                symbol: "BTCUSDT",
                instrument_id: InstrumentId::from_raw(1),
                meta,
            }],
        )
        .expect("valid decoder");

        check!().for_each(|bytes: &[u8]| {
            let mut buf = bytes.to_vec();
            let mut out = make_out();
            let _ = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
            // Must not panic — any return value is acceptable
        });
    }

    // ---- Polymarket decoder property tests ----

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "test-only setup with known-valid constants"
    )]
    fn polymarket_decoder_never_panics() {
        use mantis_polymarket::market::PolymarketMarketDecoder;
        use mantis_registry::{
            Asset, InstrumentKey, OutcomeSide, PolymarketBinding, PolymarketWindowBinding,
            Timeframe,
        };

        let mut reg = mantis_registry::InstrumentRegistry::<6>::new();
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

        let mut decoder = PolymarketMarketDecoder::<6>::new(SourceId::from_raw(10), &reg);

        check!().for_each(|bytes: &[u8]| {
            let mut buf = bytes.to_vec();
            let mut out = make_out();
            let _ = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
            // Must not panic — any return value is acceptable
        });
    }
}
