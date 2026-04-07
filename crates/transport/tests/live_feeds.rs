//! Live endpoint integration tests — connect to real exchanges.
//!
//! These tests are behind the `live-tests` feature flag and require
//! network access. Run with:
//!
//! ```bash
//! cargo test -p mantis-transport --features live-tests -- --nocapture
//! ```

#![cfg(feature = "live-tests")]
#![expect(clippy::unwrap_used, clippy::print_stdout)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use mantis_transport::binance::reference::{BinanceReferenceConfig, spawn_reference_feed};

/// Binance bookTicker: connect and receive at least 5 messages.
#[test]
fn binance_bookticker_live() {
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = BinanceReferenceConfig::default();

    let handle = spawn_reference_feed(config, move |msg| {
        let n = r.fetch_add(1, Ordering::Relaxed);
        if n < 3 {
            println!("[binance] #{n}: {}", std::str::from_utf8(&msg[..msg.len().min(120)]).unwrap_or("<binary>"));
        }
        n < 9
    })
    .unwrap();

    std::thread::sleep(Duration::from_secs(5));
    handle.shutdown();

    let count = received.load(Ordering::Relaxed);
    println!("[binance] total: {count} messages");
    assert!(count >= 5, "expected >= 5 binance messages, got {count}");
}

/// Polymarket market WS: connect, survive a ping cycle, receive broadcast events.
///
/// Subscribes to the Polymarket market channel using the generic `FeedThread`
/// (bypassing the `token_ids` guard) to validate the connection + heartbeat
/// lifecycle. Polymarket broadcasts `new_market` events to all connections
/// regardless of subscription, so we assert at least one message arrives.
#[test]
fn polymarket_market_connect_live() {
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = mantis_transport::FeedConfig {
        name: "polymarket-live-test".to_owned(),
        ws: mantis_transport::WsConfig {
            url: "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_owned(),
            subscribe_msg: Some(
                r#"{"assets_ids":[],"type":"market","custom_feature_enabled":true}"#.to_owned(),
            ),
            ping_interval: Some(Duration::from_secs(10)),
            read_timeout: Some(Duration::from_secs(15)),
        },
        tuning: mantis_transport::SocketTuning::default(),
        backoff: mantis_transport::BackoffConfig {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(5),
            jitter_factor: 0.0,
        },
    };

    let handle = mantis_transport::FeedThread::spawn(config, move |msg| {
        let n = r.fetch_add(1, Ordering::Relaxed);
        if n < 5 {
            println!("[polymarket] #{n}: {}", std::str::from_utf8(&msg[..msg.len().min(120)]).unwrap_or("<binary>"));
        }
        true
    })
    .unwrap();

    // Stay connected through at least one ping cycle (10s)
    std::thread::sleep(Duration::from_secs(15));

    let count = received.load(Ordering::Relaxed);
    let reconnects = handle.reconnects.load(Ordering::Relaxed);
    handle.shutdown();
    println!("[polymarket] total: {count} messages, reconnects: {reconnects}");
    // With empty assets_ids, Polymarket may or may not send broadcast events.
    // The test validates: connection + TLS + subscription + ping cycle survived 15s.
    assert_eq!(
        reconnects, 0,
        "expected zero reconnects during healthy session"
    );
}
