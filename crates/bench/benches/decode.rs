//! Venue decoder benchmarks.
//!
//! Measures JSON parse + normalize -> `HotEvent` pipeline for both venues.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use mantis_binance::BinanceDecoder;
use mantis_events::{EventFlags, HeartbeatPayload, HotEvent};
use mantis_fixed::FixedI64;
use mantis_registry::{
    Asset, InstrumentKey, OutcomeSide, PolymarketBinding, PolymarketWindowBinding, Timeframe,
};
use mantis_types::{InstrumentId, InstrumentMeta, SeqNum, SourceId, Timestamp};

/// Create a zeroed-out output buffer (heartbeat-initialized).
fn make_out() -> [HotEvent; 64] {
    [HotEvent::heartbeat(
        Timestamp::ZERO,
        SeqNum::ZERO,
        SourceId::from_raw(0),
        EventFlags::EMPTY,
        HeartbeatPayload { counter: 0 },
    ); 64]
}

// --- Binance Benchmarks ---

/// Construct a Binance decoder with D=3.
///
/// # Panics
///
/// Never panics -- the hard-coded constants are valid.
#[expect(
    clippy::expect_used,
    reason = "benchmark helper with known-valid constants"
)]
fn binance_decoder() -> BinanceDecoder<3> {
    let meta = InstrumentMeta::new(
        FixedI64::<3>::from_str_decimal("0.01").expect("valid tick_size"),
        FixedI64::<3>::from_str_decimal("0.001").expect("valid lot_size"),
    )
    .expect("valid meta");
    BinanceDecoder::new(SourceId::from_raw(2), InstrumentId::from_raw(1), meta)
}

fn binance_json() -> Vec<u8> {
    br#"{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1775281508123,"E":1775281508123}"#.to_vec()
}

fn bench_binance_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode/binance_book_ticker");

    group.bench_function("full_pipeline", |b| {
        let mut decoder = binance_decoder();
        b.iter(|| {
            let mut buf = binance_json();
            let mut out = make_out();
            decoder.decode(black_box(&mut buf), Timestamp::from_nanos(0), &mut out)
        });
    });

    group.finish();
}

// --- Polymarket Benchmarks ---

/// Build a leaked registry for the benchmark (lives for `'static`).
///
/// # Panics
///
/// Never panics -- the hard-coded constants are valid.
#[expect(
    clippy::expect_used,
    reason = "benchmark helper with known-valid constants"
)]
fn polymarket_registry() -> &'static mantis_registry::InstrumentRegistry<6> {
    let mut reg = mantis_registry::InstrumentRegistry::<6>::new();
    let meta = InstrumentMeta::new(
        FixedI64::<6>::from_str_decimal("0.01").expect("valid tick_size"),
        FixedI64::<6>::from_str_decimal("1.0").expect("valid lot_size"),
    )
    .expect("valid meta");
    let key = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Up);
    let id = reg
        .insert(key, meta, None, Some(PolymarketBinding::default()))
        .expect("insert ok");
    let binding = PolymarketWindowBinding {
        token_id: "abc123".to_string(),
        market_slug: "btc-up".to_string(),
        window_start: Timestamp::from_nanos(0),
        window_end: Timestamp::from_nanos(900_000_000_000),
        condition_id: None,
    };
    reg.bind_polymarket_current(id, binding).expect("bind ok");
    Box::leak(Box::new(reg))
}

fn bench_polymarket_decode(c: &mut Criterion) {
    let registry = polymarket_registry();

    let mut group = c.benchmark_group("decode/polymarket");

    // price_change (most frequent)
    group.bench_function("price_change", |b| {
        let mut decoder =
            mantis_polymarket::market::PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
        b.iter(|| {
            let mut buf = br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"BUY"}"#.to_vec();
            let mut out = make_out();
            decoder.decode(black_box(&mut buf), Timestamp::from_nanos(0), &mut out)
        });
    });

    // last_trade_price
    group.bench_function("last_trade_price", |b| {
        let mut decoder =
            mantis_polymarket::market::PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
        b.iter(|| {
            let mut buf = br#"{"type":"last_trade_price","asset_id":"abc123","price":"0.55","size":"50.0","side":"SELL"}"#.to_vec();
            let mut out = make_out();
            decoder.decode(black_box(&mut buf), Timestamp::from_nanos(0), &mut out)
        });
    });

    // book (5 levels)
    group.bench_function("book_5_levels", |b| {
        let mut decoder =
            mantis_polymarket::market::PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
        b.iter(|| {
            let mut buf = br#"{"type":"book","asset_id":"abc123","bids":[{"price":"0.50","size":"200"},{"price":"0.49","size":"150"}],"asks":[{"price":"0.55","size":"100"},{"price":"0.56","size":"80"},{"price":"0.57","size":"60"}]}"#.to_vec();
            let mut out = make_out();
            decoder.decode(black_box(&mut buf), Timestamp::from_nanos(0), &mut out)
        });
    });

    // book (20 levels)
    group.bench_function("book_20_levels", |b| {
        let mut decoder = mantis_polymarket::market::PolymarketMarketDecoder::new(
            SourceId::from_raw(1),
            registry,
        );

        let mut json = String::from(r#"{"type":"book","asset_id":"abc123","bids":["#);
        for i in 0..10 {
            if i > 0 {
                json.push(',');
            }
            let _ = write!(
                json,
                r#"{{"price":"0.{:02}","size":"{}"}}"#,
                50 - i,
                100 + i * 10
            );
        }
        json.push_str(r#"],"asks":["#);
        for i in 0..10 {
            if i > 0 {
                json.push(',');
            }
            let _ = write!(
                json,
                r#"{{"price":"0.{:02}","size":"{}"}}"#,
                55 + i,
                100 + i * 10
            );
        }
        json.push_str("]}");
        let json_bytes = json.into_bytes();

        b.iter(|| {
            let mut buf = json_bytes.clone();
            let mut out = make_out();
            decoder.decode(black_box(&mut buf), Timestamp::from_nanos(0), &mut out)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_binance_decode, bench_polymarket_decode);
criterion_main!(benches);
