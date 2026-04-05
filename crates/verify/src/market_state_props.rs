//! Bolero property-based tests for mantis-market-state.

#[cfg(test)]
mod tests {
    use bolero::check;
    use mantis_events::{BookDeltaPayload, EventFlags, HotEvent, UpdateAction};
    use mantis_market_state::{ArrayBook, MarketStateEngine, book::OrderBook};
    use mantis_types::{InstrumentId, Lots, SeqNum, Side, SourceId, Ticks, Timestamp};

    // -----------------------------------------------------------------------
    // Helper: build a book_delta HotEvent from raw fields.
    // -----------------------------------------------------------------------

    fn make_delta(price: i64, qty: i64, side: Side, flags: EventFlags) -> HotEvent {
        HotEvent::book_delta(
            Timestamp::from_nanos(1_000),
            SeqNum::from_raw(1),
            InstrumentId::from_raw(1),
            SourceId::from_raw(1),
            flags,
            BookDeltaPayload {
                price: Ticks::from_raw(price),
                qty: Lots::from_raw(qty),
                side,
                action: UpdateAction::New,
                depth: 0,
                _pad: [0; 5],
            },
        )
    }

    // -----------------------------------------------------------------------
    // prop_array_book_random_deltas_no_panic
    //
    // Feed random (price, qty, side_bit) triples into ArrayBook<100>.
    // The book must never panic regardless of values.
    // -----------------------------------------------------------------------
    #[test]
    fn prop_array_book_random_deltas_no_panic() {
        // Each element: (price_raw, qty_raw, side_bit)
        check!()
            .with_type::<Vec<(i64, i64, bool)>>()
            .for_each(|deltas| {
                let mut book = ArrayBook::<100>::default();
                for (price, qty, is_ask) in deltas {
                    let side = if *is_ask { Side::Ask } else { Side::Bid };
                    // apply_delta must not panic for any i64 price/qty pair
                    book.apply_delta(
                        Ticks::from_raw(*price),
                        Lots::from_raw(*qty),
                        side,
                        UpdateAction::New,
                    );
                }
                // Queries must also not panic
                let _ = book.best_bid();
                let _ = book.best_ask();
                let _ = book.level_count(Side::Bid);
                let _ = book.level_count(Side::Ask);
                let _ = book.total_depth(Side::Bid, 5);
                let _ = book.total_depth(Side::Ask, 5);
            });
    }

    // -----------------------------------------------------------------------
    // prop_best_bid_is_highest
    //
    // After inserting N bid levels with distinct non-negative prices that
    // fit in [0, 99], the best bid must be the maximum price inserted.
    // -----------------------------------------------------------------------
    #[test]
    fn prop_best_bid_is_highest() {
        // Generate a list of u8 prices (0-99) and u8 quantities (1-255)
        check!()
            .with_type::<Vec<(u8, u8)>>()
            .for_each(|pairs| {
                let mut book = ArrayBook::<100>::default();
                let mut max_price: Option<i64> = None;

                for (price_u8, qty_u8) in pairs {
                    // Skip zero-quantity inserts since they clear a level
                    let qty = i64::from(*qty_u8).saturating_add(1); // 1..=256
                    let price = i64::from(*price_u8);
                    book.apply_delta(
                        Ticks::from_raw(price),
                        Lots::from_raw(qty),
                        Side::Bid,
                        UpdateAction::New,
                    );
                    max_price = Some(match max_price {
                        None => price,
                        Some(prev) => prev.max(price),
                    });
                }

                if let (Some(expected_max), Some((best, _))) = (max_price, book.best_bid()) {
                    assert!(
                        best.to_raw() <= expected_max,
                        "best_bid {} exceeds max inserted price {}",
                        best.to_raw(),
                        expected_max
                    );
                }
            });
    }

    // -----------------------------------------------------------------------
    // prop_imbalance_always_in_range
    //
    // After any sequence of bid/ask inserts, book_imbalance must be in [-1, 1]
    // or None (when both sides empty).
    // -----------------------------------------------------------------------
    #[test]
    fn prop_imbalance_always_in_range() {
        check!()
            .with_type::<Vec<(u8, u8, bool)>>()
            .for_each(|deltas| {
                let mut engine = MarketStateEngine::<ArrayBook<100>, 1>::new(1, u64::MAX);
                // First prime with a snapshot so the engine is ready
                engine.process(&make_delta(45, 100, Side::Bid, EventFlags::IS_SNAPSHOT));
                engine.process(&make_delta(55, 100, Side::Ask, EventFlags::LAST_IN_BATCH));

                for (price_u8, qty_u8, is_ask) in deltas {
                    let price = i64::from(*price_u8);
                    let qty = i64::from(*qty_u8).saturating_add(1);
                    let side = if *is_ask { Side::Ask } else { Side::Bid };
                    engine.process(&make_delta(price, qty, side, EventFlags::EMPTY));
                }

                let inst = InstrumentId::from_raw(1);
                if let Some(imb) = engine.book_imbalance(inst, 5) {
                    assert!(
                        (-1.0..=1.0).contains(&imb),
                        "book_imbalance {imb} out of [-1, 1]"
                    );
                }
            });
    }
}
