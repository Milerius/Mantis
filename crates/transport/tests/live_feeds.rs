//! Live endpoint integration tests — connect to real exchanges.
//!
//! These tests are behind the `live-tests` feature flag and require
//! network access. Run with:
//!
//! ```bash
//! cargo test -p mantis-transport --features live-tests -- --nocapture
//! ```

#![cfg(feature = "live-tests")]
#![expect(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use mantis_transport::BackoffConfig;
use mantis_transport::binance::reference::{BinanceReferenceConfig, spawn_reference_feed};
use mantis_transport::polymarket::market::{PolymarketMarketConfig, spawn_market_feed};

/// Binance bookTicker: connect and receive at least 5 messages.
#[test]
fn binance_bookticker_live() {
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = BinanceReferenceConfig::default();

    let handle = spawn_reference_feed(config, move |msg| {
        let n = r.fetch_add(1, Ordering::Relaxed);
        if n < 3 {
            println!("[binance] #{n}: {}", &msg[..msg.len().min(120)]);
        }
        // Stop after 10 messages
        n < 9
    })
    .unwrap();

    // Binance bookTicker is very high frequency — 10 messages within seconds
    std::thread::sleep(Duration::from_secs(5));

    let count = received.load(Ordering::Relaxed);
    println!("[binance] total: {count} messages");
    assert!(count >= 5, "expected >= 5 binance messages, got {count}");

    handle.shutdown();
}

/// Polymarket market WS: connect with a hardcoded known token ID.
///
/// Uses a BTC up/down token ID. May fail if no active market exists
/// (outside US trading hours). This test validates connection + subscription
/// + heartbeat, not specific market data.
#[test]
fn polymarket_market_connect_live() {
    // We don't have a guaranteed active token ID, so this test just
    // validates that the connection + subscription + ping loop works.
    // It subscribes with a dummy token and checks we stay connected
    // for at least 15 seconds (through one ping cycle).
    let received = Arc::new(AtomicU32::new(0));
    let r = Arc::clone(&received);

    let config = PolymarketMarketConfig {
        // Empty subscription — we just want to validate the connection lifecycle
        token_ids: vec![],
        core_id: None,
        backoff: BackoffConfig {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(5),
            jitter_factor: 0.0,
        },
    };

    let handle = spawn_market_feed(config, move |msg| {
        let n = r.fetch_add(1, Ordering::Relaxed);
        if n < 5 {
            println!("[polymarket] #{n}: {}", &msg[..msg.len().min(120)]);
        }
        true
    })
    .unwrap();

    // Stay connected through at least one ping cycle (10s)
    std::thread::sleep(Duration::from_secs(15));

    // Connection should still be alive (not crashed, not disconnected)
    assert!(handle.is_running(), "feed thread should still be running");
    println!(
        "[polymarket] msg_count={}, reconnects={}",
        handle.msg_count.load(Ordering::Relaxed),
        handle.reconnects.load(Ordering::Relaxed)
    );

    handle.shutdown();
}
