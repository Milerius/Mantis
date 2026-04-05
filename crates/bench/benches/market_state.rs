//! Criterion benchmarks for mantis-market-state.

#![allow(clippy::unwrap_used)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use mantis_events::{BookDeltaPayload, EventFlags, HotEvent, UpdateAction};
use mantis_market_state::{ArrayBook, MarketStateEngine};
use mantis_types::{InstrumentId, Lots, SeqNum, Side, SourceId, Ticks, Timestamp};

/// Helper: construct a `HotEvent::book_delta` using the same pattern as engine tests.
fn make_delta(inst: u32, price: i64, qty: i64, side: Side, flags: EventFlags) -> HotEvent {
    HotEvent::book_delta(
        Timestamp::from_nanos(1_000),
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

/// Build a primed engine (snapshot received, book populated with bid + ask).
fn primed_engine() -> MarketStateEngine<ArrayBook<100>, 2> {
    let mut engine = MarketStateEngine::<ArrayBook<100>, 2>::new(2, 1_000_000_000);
    let snap_bid = make_delta(1, 45, 100, Side::Bid, EventFlags::IS_SNAPSHOT);
    let snap_ask = make_delta(1, 55, 200, Side::Ask, EventFlags::LAST_IN_BATCH);
    engine.process(&snap_bid);
    engine.process(&snap_ask);
    engine
}

// ---------------------------------------------------------------------------
// market_state/array_book/apply_delta
// ---------------------------------------------------------------------------

fn bench_array_book_apply_delta(c: &mut Criterion) {
    let mut group = c.benchmark_group("market_state/array_book");

    group.bench_function("apply_delta", |b| {
        use mantis_market_state::ArrayBook;
        use mantis_market_state::book::OrderBook;

        let mut book = ArrayBook::<100>::default();
        let price = Ticks::from_raw(45);
        let qty = Lots::from_raw(100);
        b.iter(|| {
            book.apply_delta(
                black_box(price),
                black_box(qty),
                black_box(Side::Bid),
                black_box(UpdateAction::New),
            );
        });
    });

    group.bench_function("best_bid", |b| {
        use mantis_market_state::ArrayBook;
        use mantis_market_state::book::OrderBook;

        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(40),
            Lots::from_raw(200),
            Side::Bid,
            UpdateAction::New,
        );
        b.iter(|| {
            black_box(book.best_bid());
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// market_state/engine/process_delta_*
// ---------------------------------------------------------------------------

fn bench_engine_process(c: &mut Criterion) {
    let mut group = c.benchmark_group("market_state/engine");

    // Mid-batch delta (no BBO check triggered)
    group.bench_function("process_delta_mid_batch", |b| {
        let mut engine = primed_engine();
        let ev = make_delta(1, 46, 150, Side::Bid, EventFlags::EMPTY);
        b.iter(|| {
            engine.process(black_box(&ev));
        });
    });

    // Batch-end delta (triggers BBO check + TopOfBook computation)
    group.bench_function("process_delta_batch_end", |b| {
        let mut engine = primed_engine();
        let ev = make_delta(1, 46, 150, Side::Bid, EventFlags::LAST_IN_BATCH);
        b.iter(|| {
            engine.process(black_box(&ev));
            // drain take_tob so each iteration starts clean
            let _ = engine.take_tob();
        });
    });

    // Micro-price query after snapshot
    group.bench_function("micro_price", |b| {
        let mut engine = primed_engine();
        let inst = InstrumentId::from_raw(1);
        b.iter(|| {
            black_box(engine.micro_price(black_box(inst)));
        });
    });

    // Book imbalance at 5 levels
    group.bench_function("book_imbalance_5", |b| {
        let mut engine = primed_engine();
        let inst = InstrumentId::from_raw(1);
        b.iter(|| {
            black_box(engine.book_imbalance(black_box(inst), black_box(5)));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_array_book_apply_delta, bench_engine_process);
criterion_main!(benches);
