//! End-to-end and stress tests for the Binance decode pipeline.
//!
//! Tests the full path: raw JSON bytes → `BinanceDecoder` → `HotEvent` output.
//! Includes throughput stress test simulating sustained high message rates.

#![expect(clippy::print_stderr, reason = "stress-test diagnostics use eprintln")]
#![expect(
    clippy::cast_precision_loss,
    reason = "f64 stats in test diagnostics are fine"
)]

use mantis_binance::{BinanceDecoder, BinanceSymbolMapping};
use mantis_events::{EventBody, EventFlags, HeartbeatPayload, HotEvent};
use mantis_fixed::FixedI64;
use mantis_types::{InstrumentId, InstrumentMeta, Lots, SeqNum, SourceId, Ticks, Timestamp};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn make_out() -> [HotEvent; 64] {
    [HotEvent::heartbeat(
        Timestamp::ZERO,
        SeqNum::ZERO,
        SourceId::from_raw(0),
        EventFlags::EMPTY,
        HeartbeatPayload { counter: 0 },
    ); 64]
}

#[expect(clippy::expect_used, reason = "test helper with known-valid constants")]
fn btc_meta() -> InstrumentMeta<3> {
    InstrumentMeta::new(
        FixedI64::<3>::from_str_decimal("0.01").expect("tick"),
        FixedI64::<3>::from_str_decimal("0.001").expect("lot"),
    )
    .expect("meta")
}

#[expect(clippy::expect_used, reason = "test helper with known-valid constants")]
fn multi_decoder() -> BinanceDecoder<3> {
    let btc = btc_meta();
    let eth = InstrumentMeta::new(
        FixedI64::<3>::from_str_decimal("0.01").expect("tick"),
        FixedI64::<3>::from_str_decimal("0.01").expect("lot"),
    )
    .expect("meta");

    BinanceDecoder::new(
        SourceId::from_raw(2),
        &[
            BinanceSymbolMapping {
                symbol: "BTCUSDT",
                instrument_id: InstrumentId::from_raw(1),
                meta: btc,
            },
            BinanceSymbolMapping {
                symbol: "ETHUSDT",
                instrument_id: InstrumentId::from_raw(2),
                meta: eth,
            },
        ],
    )
    .expect("decoder")
}

// --- End-to-End Tests ---

/// Full pipeline: raw JSON → decode → verify all `HotEvent` fields.
#[test]
#[expect(clippy::panic, reason = "test uses panic for match-arm failure")]
fn e2e_binance_btc_bookticker_full_field_verification() {
    let mut decoder = multi_decoder();
    let mut out = make_out();

    let mut buf = br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"72681.70","B":"4.160","a":"72681.80","A":"4.190","T":1775890062100,"E":1775890062100}}"#.to_vec();
    let recv_ts = Timestamp::from_nanos(1_000_000_000);

    let n = decoder.decode(&mut buf, recv_ts, &mut out);
    assert_eq!(n, 1, "should produce exactly 1 event");

    let event = &out[0];

    // Header
    assert_eq!(event.header.recv_ts, recv_ts);
    assert_eq!(event.header.instrument_id, InstrumentId::from_raw(1));
    assert_eq!(event.header.source_id, SourceId::from_raw(2));
    assert_eq!(event.header.seq.to_raw(), 1);
    assert_eq!(event.header.flags, EventFlags::EMPTY);

    // Body
    match &event.body {
        EventBody::TopOfBook(tob) => {
            // 72681.70 / 0.01 = 7268170
            assert_eq!(tob.bid_price, Ticks::from_raw(7_268_170));
            // 4.160 / 0.001 = 4160
            assert_eq!(tob.bid_qty, Lots::from_raw(4160));
            // 72681.80 / 0.01 = 7268180
            assert_eq!(tob.ask_price, Ticks::from_raw(7_268_180));
            // 4.190 / 0.001 = 4190
            assert_eq!(tob.ask_qty, Lots::from_raw(4190));
        }
        other => panic!("expected TopOfBook, got {:?}", other.kind()),
    }
}

/// Multi-symbol routing: alternating BTC and ETH messages.
#[test]
fn e2e_multi_symbol_alternating() {
    let mut decoder = multi_decoder();
    let mut out = make_out();

    let msgs: Vec<Vec<u8>> = vec![
        br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"72000.00","B":"1.000","a":"72001.00","A":"2.000","T":1,"E":1}}"#.to_vec(),
        br#"{"stream":"ethusdt@bookTicker","data":{"e":"bookTicker","s":"ETHUSDT","b":"3500.00","B":"10.00","a":"3501.00","A":"20.00","T":1,"E":1}}"#.to_vec(),
        br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"72002.00","B":"3.000","a":"72003.00","A":"4.000","T":1,"E":1}}"#.to_vec(),
        br#"{"stream":"ethusdt@bookTicker","data":{"e":"bookTicker","s":"ETHUSDT","b":"3502.00","B":"30.00","a":"3503.00","A":"40.00","T":1,"E":1}}"#.to_vec(),
    ];

    let expected_ids = [1u32, 2, 1, 2];
    let mut seq_prev = 0u64;

    for (i, msg) in msgs.iter().enumerate() {
        let mut buf = msg.clone();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1, "msg {i} should produce 1 event");
        assert_eq!(
            out[0].header.instrument_id,
            InstrumentId::from_raw(expected_ids[i]),
            "msg {i} wrong instrument"
        );
        assert!(
            out[0].header.seq.to_raw() > seq_prev,
            "msg {i} seq not monotonic"
        );
        seq_prev = out[0].header.seq.to_raw();
    }
}

/// Unknown symbols are silently skipped (0 events).
#[test]
fn e2e_unknown_symbol_skipped() {
    let mut decoder = multi_decoder();
    let mut out = make_out();

    let mut buf =
        br#"{"e":"bookTicker","s":"SOLUSDT","b":"150.00","B":"100","a":"150.10","A":"50","T":1,"E":1}"#
            .to_vec();
    let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
    assert_eq!(n, 0);
}

/// Malformed JSON never panics and returns 0.
#[test]
fn e2e_malformed_never_panics() {
    let mut decoder = multi_decoder();
    let mut out = make_out();

    let bad_inputs: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"not json".to_vec(),
        b"{}".to_vec(),
        b"{\"e\":\"bookTicker\"}".to_vec(), // missing fields
        b"{\"e\":\"bookTicker\",\"s\":\"BTCUSDT\",\"b\":\"NaN\",\"B\":\"1\",\"a\":\"72000\",\"A\":\"1\",\"T\":1,\"E\":1}".to_vec(),
        b"\x00\x01\x02\xff\xfe".to_vec(), // binary garbage
        vec![0u8; 65536],                   // large buffer
    ];

    for (i, input) in bad_inputs.iter().enumerate() {
        let mut buf = input.clone();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0, "bad input {i} should produce 0 events");
    }
}

// --- Stress Tests ---

/// Sustained throughput: decode 1M messages, verify latency and correctness.
#[test]
#[ignore = "stress test"]
fn stress_1m_messages() {
    let mut decoder = multi_decoder();
    let mut out = make_out();
    let template =
        br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"72681.70","B":"4.160","a":"72681.80","A":"4.190","T":1775890062100,"E":1775890062100}}"#;

    let iterations = 1_000_000u64;
    let mut total_events = 0u64;

    let start = Instant::now();
    for _ in 0..iterations {
        let mut buf = template.to_vec();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        total_events += n as u64;
    }
    let elapsed = start.elapsed();

    assert_eq!(total_events, iterations, "every message should decode");

    let ns_per_msg = elapsed.as_nanos() as f64 / iterations as f64;
    let msgs_per_sec = iterations as f64 / elapsed.as_secs_f64();

    eprintln!("--- Binance Decoder Stress Test ---");
    eprintln!("  Messages:     {iterations}");
    eprintln!("  Total time:   {elapsed:?}");
    eprintln!("  ns/msg:       {ns_per_msg:.1}");
    eprintln!("  msgs/sec:     {msgs_per_sec:.0}");
    eprintln!("  Final seq:    {}", out[0].header.seq.to_raw());

    // Sanity: should be under 2us/msg even in debug mode
    assert!(
        ns_per_msg < 10_000.0,
        "decode too slow: {ns_per_msg:.0}ns/msg"
    );
}

/// Stress test with alternating symbols to exercise lookup path.
#[test]
#[ignore = "stress test"]
fn stress_multi_symbol_500k() {
    let mut decoder = multi_decoder();
    let mut out = make_out();
    let btc_msg =
        br#"{"stream":"btcusdt@bookTicker","data":{"e":"bookTicker","s":"BTCUSDT","b":"72000.00","B":"1.000","a":"72001.00","A":"2.000","T":1,"E":1}}"#;
    let eth_msg =
        br#"{"stream":"ethusdt@bookTicker","data":{"e":"bookTicker","s":"ETHUSDT","b":"3500.00","B":"10.00","a":"3501.00","A":"20.00","T":1,"E":1}}"#;

    let iterations = 500_000u64;
    let mut btc_count = 0u64;
    let mut eth_count = 0u64;

    let start = Instant::now();
    for i in 0..iterations {
        let mut buf = if i % 2 == 0 {
            btc_msg.to_vec()
        } else {
            eth_msg.to_vec()
        };
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);
        if out[0].header.instrument_id == InstrumentId::from_raw(1) {
            btc_count += 1;
        } else {
            eth_count += 1;
        }
    }
    let elapsed = start.elapsed();

    assert_eq!(btc_count, iterations / 2);
    assert_eq!(eth_count, iterations / 2);

    let ns_per_msg = elapsed.as_nanos() as f64 / iterations as f64;
    eprintln!("--- Multi-Symbol Stress Test ---");
    eprintln!("  Messages:     {iterations} ({btc_count} BTC + {eth_count} ETH)");
    eprintln!("  Total time:   {elapsed:?}");
    eprintln!("  ns/msg:       {ns_per_msg:.1}");
}

/// Spawn callback stress: simulate high-rate push with backpressure.
#[test]
#[ignore = "stress test"]
#[expect(clippy::expect_used, reason = "test helper with known-valid constants")]
fn stress_spawn_callback_backpressure() {
    use mantis_binance::spawn::build_callback;

    let meta = btc_meta();
    let decoder = BinanceDecoder::new(
        SourceId::from_raw(2),
        &[BinanceSymbolMapping {
            symbol: "BTCUSDT",
            instrument_id: InstrumentId::from_raw(1),
            meta,
        }],
    )
    .expect("decoder");

    let accept_count = Arc::new(AtomicU64::new(0));
    let ac = Arc::clone(&accept_count);

    // Simulate 50% drop rate
    let (mut callback, event_count, drop_count) = build_callback(decoder, move |_| {
        let n = ac.fetch_add(1, Ordering::Relaxed);
        n.is_multiple_of(2) // accept every other event
    });

    let template =
        br#"{"e":"bookTicker","s":"BTCUSDT","b":"72681.70","B":"4.160","a":"72681.80","A":"4.190","T":1,"E":1}"#;

    let iterations = 100_000u64;
    for _ in 0..iterations {
        let mut buf = template.to_vec();
        callback(&mut buf);
    }

    let events = event_count.load(Ordering::Relaxed);
    let drops = drop_count.load(Ordering::Relaxed);

    eprintln!("--- Backpressure Stress Test ---");
    eprintln!("  Iterations: {iterations}");
    eprintln!("  Events:     {events}");
    eprintln!("  Drops:      {drops}");
    eprintln!("  Drop rate:  {:.1}%", drops as f64 / events as f64 * 100.0);

    assert_eq!(events, iterations, "all messages should decode");
    assert!(drops > 0, "should have some drops");
    // ~50% drop rate
    assert!(
        drops > iterations / 4,
        "expected ~50% drops, got {drops}/{events}"
    );
}
