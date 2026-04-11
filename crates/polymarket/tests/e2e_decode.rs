//! End-to-end and stress tests for the Polymarket decode pipeline.
//!
//! Tests the full path: raw JSON bytes → `PolymarketMarketDecoder` → `HotEvent` output.
//! Includes stress tests for sustained message rates and batch book snapshots.

#![expect(clippy::print_stderr, reason = "stress-test diagnostics use eprintln")]
#![expect(
    clippy::cast_precision_loss,
    reason = "f64 stats in test diagnostics are fine"
)]

use mantis_events::{EventBody, EventFlags, HeartbeatPayload, HotEvent};
use mantis_fixed::FixedI64;
use mantis_polymarket::market::PolymarketMarketDecoder;
use mantis_registry::*;
use mantis_types::{InstrumentId, InstrumentMeta, Lots, SeqNum, Side, SourceId, Ticks, Timestamp};
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

#[expect(
    clippy::expect_used,
    reason = "test-only helper with known-valid constants"
)]
fn test_registry() -> &'static InstrumentRegistry<6> {
    let mut reg = InstrumentRegistry::<6>::new();
    let meta = InstrumentMeta::new(
        FixedI64::<6>::from_raw(10_000),    // tick_size = 0.01
        FixedI64::<6>::from_raw(1_000_000), // lot_size = 1.0
    )
    .expect("valid meta");

    // BTC-M15-Up
    let key_up = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Up);
    let id_up = reg
        .insert(key_up, meta, None, Some(PolymarketBinding::default()))
        .expect("insert up");
    reg.bind_polymarket_current(
        id_up,
        PolymarketWindowBinding {
            token_id: "token_up_123".to_owned(),
            market_slug: "btc-15m-up".to_owned(),
            window_start: Timestamp::from_nanos(0),
            window_end: Timestamp::from_nanos(900_000_000_000),
            condition_id: None,
        },
    )
    .expect("bind up");

    // BTC-M15-Down
    let key_down = InstrumentKey::prediction(Asset::Btc, Timeframe::M15, OutcomeSide::Down);
    let id_down = reg
        .insert(key_down, meta, None, Some(PolymarketBinding::default()))
        .expect("insert down");
    reg.bind_polymarket_current(
        id_down,
        PolymarketWindowBinding {
            token_id: "token_down_456".to_owned(),
            market_slug: "btc-15m-down".to_owned(),
            window_start: Timestamp::from_nanos(0),
            window_end: Timestamp::from_nanos(900_000_000_000),
            condition_id: None,
        },
    )
    .expect("bind down");

    Box::leak(Box::new(reg))
}

// --- End-to-End Tests ---

/// Full pipeline: `price_change` → `BookDelta` with all fields verified.
#[test]
#[expect(clippy::panic, reason = "test uses panic for match-arm failure")]
fn e2e_price_change_full_verification() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let mut buf =
        br#"{"type":"price_change","asset_id":"token_up_123","price":"0.53","size":"250.0","side":"BUY"}"#
            .to_vec();
    let recv_ts = Timestamp::from_nanos(1_000_000_000);
    let n = decoder.decode(&mut buf, recv_ts, &mut out);

    assert_eq!(n, 1);
    let event = &out[0];
    assert_eq!(event.header.recv_ts, recv_ts);
    assert_eq!(event.header.source_id, SourceId::from_raw(1));
    assert_eq!(event.header.instrument_id, InstrumentId::from_raw(1));

    match &event.body {
        EventBody::BookDelta(bd) => {
            assert_eq!(bd.price, Ticks::from_raw(53)); // 0.53 / 0.01
            assert_eq!(bd.qty, Lots::from_raw(250)); // 250.0 / 1.0
            assert_eq!(bd.side, Side::Bid);
        }
        other => panic!("expected BookDelta, got {:?}", other.kind()),
    }
}

/// Multi-token routing: Up and Down tokens map to different instruments.
#[test]
fn e2e_multi_token_routing() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    // Up token
    let mut buf_up =
        br#"{"type":"price_change","asset_id":"token_up_123","price":"0.60","size":"100.0","side":"BUY"}"#
            .to_vec();
    let n = decoder.decode(&mut buf_up, Timestamp::from_nanos(0), &mut out);
    assert_eq!(n, 1);
    assert_eq!(out[0].header.instrument_id, InstrumentId::from_raw(1));

    // Down token
    let mut buf_down =
        br#"{"type":"price_change","asset_id":"token_down_456","price":"0.40","size":"200.0","side":"SELL"}"#
            .to_vec();
    let n = decoder.decode(&mut buf_down, Timestamp::from_nanos(0), &mut out);
    assert_eq!(n, 1);
    assert_eq!(out[0].header.instrument_id, InstrumentId::from_raw(2));
}

/// Book snapshot produces batch with `LAST_IN_BATCH` on final event.
#[test]
#[expect(clippy::panic, reason = "test uses panic for match-arm failure")]
fn e2e_book_snapshot_batch() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let mut buf = br#"{"type":"book","asset_id":"token_up_123","bids":[{"price":"0.50","size":"100"},{"price":"0.49","size":"200"}],"asks":[{"price":"0.55","size":"150"},{"price":"0.56","size":"80"}]}"#.to_vec();

    let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
    assert_eq!(n, 4);

    // All should have IS_SNAPSHOT
    for (i, ev) in out[..4].iter().enumerate() {
        assert!(
            ev.header.flags.contains(EventFlags::IS_SNAPSHOT),
            "event {i} missing IS_SNAPSHOT"
        );
    }

    // Only last should have LAST_IN_BATCH
    for (i, ev) in out[..3].iter().enumerate() {
        assert!(
            !ev.header.flags.contains(EventFlags::LAST_IN_BATCH),
            "event {i} should NOT have LAST_IN_BATCH"
        );
    }
    assert!(
        out[3].header.flags.contains(EventFlags::LAST_IN_BATCH),
        "last event should have LAST_IN_BATCH"
    );

    // Verify sides
    assert_eq!(
        match &out[0].body {
            EventBody::BookDelta(bd) => bd.side,
            _ => panic!("expected BookDelta"),
        },
        Side::Bid
    );
    assert_eq!(
        match &out[2].body {
            EventBody::BookDelta(bd) => bd.side,
            _ => panic!("expected BookDelta"),
        },
        Side::Ask
    );
}

/// Trade event from `last_trade_price`.
#[test]
#[expect(clippy::panic, reason = "test uses panic for match-arm failure")]
fn e2e_trade_event() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let mut buf =
        br#"{"type":"last_trade_price","asset_id":"token_up_123","price":"0.55","size":"50.0","side":"SELL"}"#
            .to_vec();
    let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
    assert_eq!(n, 1);

    match &out[0].body {
        EventBody::Trade(t) => {
            assert_eq!(t.price, Ticks::from_raw(55));
            assert_eq!(t.qty, Lots::from_raw(50));
            assert_eq!(t.aggressor, Side::Ask);
        }
        other => panic!("expected Trade, got {:?}", other.kind()),
    }
}

/// Non-hot message types are skipped.
#[test]
fn e2e_non_hot_types_skipped() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    for msg_type in &[
        "best_bid_ask",
        "tick_size_change",
        "market_resolved",
        "new_market",
    ] {
        let mut buf = format!(r#"{{"type":"{msg_type}"}}"#).into_bytes();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0, "{msg_type} should be skipped");
    }
}

/// Unknown token returns 0 events.
#[test]
fn e2e_unknown_token() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let mut buf =
        br#"{"type":"price_change","asset_id":"unknown_token_xyz","price":"0.50","size":"100","side":"BUY"}"#
            .to_vec();
    let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
    assert_eq!(n, 0);
}

/// Malformed inputs never panic.
#[test]
fn e2e_malformed_never_panics() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let bad: Vec<Vec<u8>> = vec![
        b"".to_vec(),
        b"not json".to_vec(),
        b"{}".to_vec(),
        b"{\"type\":\"price_change\"}".to_vec(),
        b"\x00\xff\xfe".to_vec(),
        vec![b'{'; 65536],
    ];

    for (i, input) in bad.iter().enumerate() {
        let mut buf = input.clone();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 0, "bad input {i} should produce 0 events");
    }
}

// --- Stress Tests ---

/// 1M `price_change` messages sustained throughput.
#[test]
#[ignore = "stress test"]
fn stress_1m_price_changes() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let template =
        br#"{"type":"price_change","asset_id":"token_up_123","price":"0.53","size":"100.0","side":"BUY"}"#;

    let iterations = 1_000_000u64;
    let mut total_events = 0u64;

    let start = Instant::now();
    for _ in 0..iterations {
        let mut buf = template.to_vec();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        total_events += n as u64;
    }
    let elapsed = start.elapsed();

    assert_eq!(total_events, iterations);

    let ns_per_msg = elapsed.as_nanos() as f64 / iterations as f64;
    let msgs_per_sec = iterations as f64 / elapsed.as_secs_f64();

    eprintln!("--- Polymarket Price Change Stress Test ---");
    eprintln!("  Messages:     {iterations}");
    eprintln!("  Total time:   {elapsed:?}");
    eprintln!("  ns/msg:       {ns_per_msg:.1}");
    eprintln!("  msgs/sec:     {msgs_per_sec:.0}");

    assert!(
        ns_per_msg < 10_000.0,
        "decode too slow: {ns_per_msg:.0}ns/msg"
    );
}

/// Alternating Up/Down tokens for multi-instrument stress.
#[test]
#[ignore = "stress test"]
fn stress_500k_alternating_tokens() {
    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    let up_msg =
        br#"{"type":"price_change","asset_id":"token_up_123","price":"0.60","size":"100","side":"BUY"}"#;
    let down_msg =
        br#"{"type":"price_change","asset_id":"token_down_456","price":"0.40","size":"100","side":"SELL"}"#;

    let iterations = 500_000u64;
    let mut up_count = 0u64;
    let mut down_count = 0u64;

    let start = Instant::now();
    for i in 0..iterations {
        let mut buf = if i % 2 == 0 {
            up_msg.to_vec()
        } else {
            down_msg.to_vec()
        };
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 1);
        if out[0].header.instrument_id == InstrumentId::from_raw(1) {
            up_count += 1;
        } else {
            down_count += 1;
        }
    }
    let elapsed = start.elapsed();

    assert_eq!(up_count, iterations / 2);
    assert_eq!(down_count, iterations / 2);

    let ns_per_msg = elapsed.as_nanos() as f64 / iterations as f64;
    eprintln!("--- Polymarket Multi-Token Stress Test ---");
    eprintln!("  Messages:     {iterations} ({up_count} Up + {down_count} Down)");
    eprintln!("  Total time:   {elapsed:?}");
    eprintln!("  ns/msg:       {ns_per_msg:.1}");
}

/// Book snapshot stress: 10K snapshots with 20 levels each.
#[test]
#[ignore = "stress test"]
#[expect(clippy::expect_used, reason = "write!() to String is infallible")]
fn stress_10k_book_snapshots() {
    use std::fmt::Write;

    let registry = test_registry();
    let mut decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);
    let mut out = make_out();

    // Build a 20-level snapshot
    let mut json = String::from(r#"{"type":"book","asset_id":"token_up_123","bids":["#);
    for i in 0..10 {
        if i > 0 {
            json.push(',');
        }
        write!(
            json,
            r#"{{"price":"0.{:02}","size":"{}"}}"#,
            50 - i,
            100 + i * 10
        )
        .expect("write to String");
    }
    json.push_str(r#"],"asks":["#);
    for i in 0..10 {
        if i > 0 {
            json.push(',');
        }
        write!(
            json,
            r#"{{"price":"0.{:02}","size":"{}"}}"#,
            55 + i,
            100 + i * 10
        )
        .expect("write to String");
    }
    json.push_str("]}");
    let template = json.into_bytes();

    let iterations = 10_000u64;
    let mut total_events = 0u64;

    let start = Instant::now();
    for _ in 0..iterations {
        let mut buf = template.clone();
        let n = decoder.decode(&mut buf, Timestamp::from_nanos(0), &mut out);
        assert_eq!(n, 20);
        total_events += n as u64;
    }
    let elapsed = start.elapsed();

    assert_eq!(total_events, iterations * 20);

    let ns_per_snapshot = elapsed.as_nanos() as f64 / iterations as f64;
    let ns_per_event = elapsed.as_nanos() as f64 / total_events as f64;
    eprintln!("--- Polymarket Book Snapshot Stress Test ---");
    eprintln!("  Snapshots:    {iterations} (20 levels each)");
    eprintln!("  Total events: {total_events}");
    eprintln!("  Total time:   {elapsed:?}");
    eprintln!("  ns/snapshot:  {ns_per_snapshot:.1}");
    eprintln!("  ns/event:     {ns_per_event:.1}");
}

/// Spawn callback with backpressure simulation.
#[test]
#[ignore = "stress test"]
fn stress_spawn_callback_backpressure() {
    use mantis_polymarket::market::spawn::build_callback;

    let registry = test_registry();
    let decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);

    let accept_count = Arc::new(AtomicU64::new(0));
    let ac = Arc::clone(&accept_count);

    let (mut callback, event_count, drop_count) = build_callback(decoder, move |_| {
        let n = ac.fetch_add(1, Ordering::Relaxed);
        !n.is_multiple_of(3) // drop every 3rd event (~33% drop rate)
    });

    let template =
        br#"{"type":"price_change","asset_id":"token_up_123","price":"0.53","size":"100","side":"BUY"}"#;

    let iterations = 100_000u64;
    for _ in 0..iterations {
        let mut buf = template.to_vec();
        callback(&mut buf);
    }

    let events = event_count.load(Ordering::Relaxed);
    let drops = drop_count.load(Ordering::Relaxed);

    eprintln!("--- Polymarket Backpressure Stress Test ---");
    eprintln!("  Iterations: {iterations}");
    eprintln!("  Events:     {events}");
    eprintln!("  Drops:      {drops}");
    eprintln!("  Drop rate:  {:.1}%", drops as f64 / events as f64 * 100.0);

    assert_eq!(events, iterations);
    assert!(drops > 0);
    assert!(
        drops > iterations / 5,
        "expected ~33% drops, got {drops}/{events}"
    );
}
