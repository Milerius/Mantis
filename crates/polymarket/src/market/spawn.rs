//! Convenience functions for spawning Polymarket market feed threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mantis_events::HotEvent;
use mantis_registry::InstrumentRegistry;
use mantis_transport::FeedHandle;
use mantis_transport::polymarket::market::PolymarketMarketConfig;
use mantis_types::{SourceId, Timestamp};

use crate::market::decoder::PolymarketMarketDecoder;

/// Result of spawning a Polymarket market feed.
pub struct FeedSpawnResult {
    /// Handle for shutdown and monitoring.
    pub handle: FeedHandle,
    /// Counter of successfully decoded events (for `FeedMonitor` registration).
    pub event_count: Arc<AtomicU64>,
    /// Counter of events dropped due to push returning false.
    pub drop_count: Arc<AtomicU64>,
}

/// Build the transport callback (exposed for testing).
pub fn build_callback<const D: u8, F>(
    mut decoder: PolymarketMarketDecoder<'static, D>,
    mut push: F,
) -> (
    impl FnMut(&mut [u8]) -> bool,
    Arc<AtomicU64>,
    Arc<AtomicU64>,
)
where
    F: FnMut(HotEvent) -> bool + Send + 'static,
{
    let event_count = Arc::new(AtomicU64::new(0));
    let drop_count = Arc::new(AtomicU64::new(0));
    let ec = Arc::clone(&event_count);
    let dc = Arc::clone(&drop_count);

    let mut out = make_out();
    let callback = move |buf: &mut [u8]| {
        let recv_ts = Timestamp::now();
        let count = decoder.decode(buf, recv_ts, &mut out);
        if count > 0 {
            ec.fetch_add(count as u64, Ordering::Relaxed);
        }
        for event in out.iter().take(count) {
            if !push(*event) {
                dc.fetch_add(1, Ordering::Relaxed);
            }
        }
        true
    };

    (callback, event_count, drop_count)
}

/// Spawn a Polymarket market feed thread with built-in decoder.
///
/// # Errors
///
/// Returns an error if the OS fails to spawn the feed thread or if no
/// token IDs are configured.
pub fn spawn_polymarket_market_feed<const D: u8, F>(
    config: PolymarketMarketConfig,
    source_id: SourceId,
    registry: &'static InstrumentRegistry<D>,
    push: F,
) -> Result<FeedSpawnResult, std::io::Error>
where
    F: FnMut(HotEvent) -> bool + Send + 'static,
{
    let decoder = PolymarketMarketDecoder::new(source_id, registry);
    let (callback, event_count, drop_count) = build_callback(decoder, push);
    let handle = mantis_transport::polymarket::market::spawn_market_feed(config, callback)?;
    Ok(FeedSpawnResult {
        handle,
        event_count,
        drop_count,
    })
}

fn make_out() -> [HotEvent; 64] {
    [HotEvent::heartbeat(
        Timestamp::ZERO,
        mantis_types::SeqNum::ZERO,
        mantis_types::SourceId::from_raw(0),
        mantis_events::EventFlags::EMPTY,
        mantis_events::HeartbeatPayload { counter: 0 },
    ); 64]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mantis_fixed::FixedI64;
    use mantis_registry::*;
    use mantis_types::{InstrumentMeta, Timestamp};

    /// Create a leaked static registry with one BTC-M15-Up instrument.
    /// Follows the exact pattern from polymarket decoder tests.
    #[expect(clippy::expect_used, reason = "test-only helper")]
    fn test_registry() -> &'static InstrumentRegistry<6> {
        let mut reg = InstrumentRegistry::<6>::new();
        let meta = InstrumentMeta::new(
            FixedI64::<6>::from_raw(10_000),    // tick_size = 0.01
            FixedI64::<6>::from_raw(1_000_000), // lot_size = 1.0
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
        Box::leak(Box::new(reg))
    }

    #[test]
    fn callback_decodes_and_pushes() {
        let registry = test_registry();
        let decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);

        let push_count = Arc::new(AtomicU64::new(0));
        let pc = Arc::clone(&push_count);

        let (mut callback, event_count, drop_count) = build_callback(decoder, move |_| {
            pc.fetch_add(1, Ordering::Relaxed);
            true
        });

        let mut buf = br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"BUY"}"#.to_vec();
        assert!(callback(&mut buf));
        assert_eq!(push_count.load(Ordering::Relaxed), 1);
        assert_eq!(event_count.load(Ordering::Relaxed), 1);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn callback_counts_drops() {
        let registry = test_registry();
        let decoder = PolymarketMarketDecoder::new(SourceId::from_raw(1), registry);

        let (mut callback, event_count, drop_count) = build_callback(decoder, |_| false);

        let mut buf = br#"{"type":"price_change","asset_id":"abc123","price":"0.53","size":"100.0","side":"BUY"}"#.to_vec();
        callback(&mut buf);
        assert_eq!(event_count.load(Ordering::Relaxed), 1);
        assert_eq!(drop_count.load(Ordering::Relaxed), 1);
    }
}
