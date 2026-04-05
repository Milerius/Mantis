//! Multi-instrument stress test.
//! 12 instruments, interleaved events, verify independent book state.

use mantis_events::{BookDeltaPayload, EventFlags, HotEvent, UpdateAction};
use mantis_market_state::{ArrayBook, MarketStateEngine, OrderBook};
use mantis_types::{InstrumentId, Lots, SeqNum, Side, SourceId, Ticks, Timestamp};

fn make_delta(inst: u32, price: i64, qty: i64, side: Side, flags: EventFlags) -> HotEvent {
    HotEvent::book_delta(
        Timestamp::from_nanos(1000),
        SeqNum::from_raw(1),
        InstrumentId::from_raw(inst),
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

#[test]
fn twelve_instruments_independent() {
    let mut engine = MarketStateEngine::<ArrayBook<100>, 12>::new(12, 1_000_000_000);

    // Initialize each instrument with a unique bid/ask
    for i in 1..=12u32 {
        let bid_price = (i * 5) as i64; // 5, 10, 15, ..., 60
        let ask_price = (i * 5 + 2) as i64; // 7, 12, 17, ..., 62
        let snap_bid = make_delta(i, bid_price, 100, Side::Bid, EventFlags::IS_SNAPSHOT);
        let snap_ask = make_delta(i, ask_price, 200, Side::Ask, EventFlags::LAST_IN_BATCH);
        engine.process(&snap_bid);
        engine.process(&snap_ask);
    }

    // Verify each instrument has correct independent state
    for i in 1..=12u32 {
        let inst = InstrumentId::from_raw(i);
        let bid_price = (i * 5) as i64;
        let ask_price = (i * 5 + 2) as i64;

        assert!(engine.is_ready(inst), "inst {i} should be ready");
        let book = engine.book(inst).unwrap();
        assert_eq!(
            book.best_bid(),
            Some((Ticks::from_raw(bid_price), Lots::from_raw(100)))
        );
        assert_eq!(
            book.best_ask(),
            Some((Ticks::from_raw(ask_price), Lots::from_raw(200)))
        );

        let mp = engine.micro_price(inst).unwrap();
        assert!(
            mp.to_raw() >= bid_price && mp.to_raw() <= ask_price,
            "micro_price {mp:?} out of bid/ask range [{bid_price}, {ask_price}] for inst {i}"
        );
    }
}

#[test]
fn high_throughput_stress() {
    let mut engine = MarketStateEngine::<ArrayBook<100>, 4>::new(4, 1_000_000_000);

    // Snapshot all 4 instruments
    for i in 1..=4u32 {
        engine.process(&make_delta(i, 40, 100, Side::Bid, EventFlags::IS_SNAPSHOT));
        engine.process(&make_delta(i, 60, 100, Side::Ask, EventFlags::LAST_IN_BATCH));
    }

    // Process 100K interleaved deltas
    let mut tob_count = 0;
    for j in 0..100_000u64 {
        let inst = (j % 4 + 1) as u32;
        let price = 40 + (j % 20) as i64;
        let side = if j % 2 == 0 { Side::Bid } else { Side::Ask };
        let flags = if j % 10 == 9 {
            EventFlags::LAST_IN_BATCH
        } else {
            EventFlags::EMPTY
        };
        engine.process(&make_delta(inst, price, 50, side, flags));
        if engine.take_tob().is_some() {
            tob_count += 1;
        }
    }

    assert!(tob_count > 0, "should have emitted some TopOfBook events");
    assert!(
        tob_count < 100_000,
        "should not emit on every event, got {tob_count}"
    );

    // All instruments should still be ready
    for i in 1..=4u32 {
        assert!(
            engine.is_ready(InstrumentId::from_raw(i)),
            "inst {i} should still be ready after stress"
        );
    }
}
