//! Convenience functions for spawning Binance feed threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mantis_events::HotEvent;
use mantis_transport::FeedHandle;
use mantis_transport::binance::reference::BinanceReferenceConfig;
use mantis_types::Timestamp;

use crate::decoder::BinanceDecoder;

/// Result of spawning a Binance feed.
pub struct FeedSpawnResult {
    /// Handle for shutdown and monitoring.
    pub handle: FeedHandle,
    /// Counter of successfully decoded events (for `FeedMonitor` registration).
    pub event_count: Arc<AtomicU64>,
    /// Counter of events dropped due to push returning false.
    pub drop_count: Arc<AtomicU64>,
}

/// Build the transport callback closure.
///
/// Returns `(callback, event_count, drop_count)`.
/// Exposed for unit testing without a real WebSocket.
pub fn build_callback<const D: u8, F>(
    mut decoder: BinanceDecoder<D>,
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
        // Always count — proves the feed is alive even for non-hot messages
        ec.fetch_add(1, Ordering::Relaxed);
        let recv_ts = Timestamp::now();
        let count = decoder.decode(buf, recv_ts, &mut out);
        for event in out.iter().take(count) {
            if !push(*event) {
                dc.fetch_add(1, Ordering::Relaxed);
            }
        }
        true // never stop the feed
    };

    (callback, event_count, drop_count)
}

/// Spawn a Binance reference feed thread with built-in decoder.
///
/// # Errors
///
/// Returns an error if the OS fails to spawn the thread.
pub fn spawn_binance_feed<const D: u8, F>(
    config: BinanceReferenceConfig,
    mut decoder: BinanceDecoder<D>,
    push: F,
) -> Result<FeedSpawnResult, std::io::Error>
where
    F: FnMut(HotEvent) -> bool + Send + 'static,
{
    // Sync decoder stream mode with transport config so combined-stream
    // JSON is always decoded correctly regardless of mapping count.
    decoder.set_combined_stream(config.symbols.len() > 1);
    let (callback, event_count, drop_count) = build_callback(decoder, push);
    let handle = mantis_transport::binance::reference::spawn_reference_feed(config, callback)?;
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
    use mantis_types::{InstrumentId, InstrumentMeta, SourceId};

    use crate::BinanceSymbolMapping;

    #[expect(clippy::expect_used, reason = "test-only helper")]
    fn test_decoder() -> BinanceDecoder<3> {
        let meta = InstrumentMeta::new(
            FixedI64::<3>::from_str_decimal("0.01").expect("tick"),
            FixedI64::<3>::from_str_decimal("0.001").expect("lot"),
        )
        .expect("meta");
        BinanceDecoder::new(
            SourceId::from_raw(2),
            &[BinanceSymbolMapping {
                symbol: "BTCUSDT",
                instrument_id: InstrumentId::from_raw(1),
                meta,
            }],
        )
        .expect("decoder")
    }

    #[test]
    fn callback_decodes_and_pushes() {
        let decoder = test_decoder();
        let push_count = Arc::new(AtomicU64::new(0));
        let pc = Arc::clone(&push_count);

        let (mut callback, event_count, drop_count) = build_callback(decoder, move |_| {
            pc.fetch_add(1, Ordering::Relaxed);
            true
        });

        let mut buf = br#"{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1,"E":1}"#.to_vec();
        assert!(callback(&mut buf));
        assert_eq!(push_count.load(Ordering::Relaxed), 1);
        assert_eq!(event_count.load(Ordering::Relaxed), 1);
        assert_eq!(drop_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn callback_counts_drops() {
        let decoder = test_decoder();
        let (mut callback, event_count, drop_count) = build_callback(decoder, |_| false);

        let mut buf = br#"{"e":"bookTicker","s":"BTCUSDT","b":"67396.70","B":"8.819","a":"67396.90","A":"7.181","T":1,"E":1}"#.to_vec();
        callback(&mut buf);
        assert_eq!(event_count.load(Ordering::Relaxed), 1);
        assert_eq!(drop_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn callback_skips_malformed() {
        let decoder = test_decoder();
        let (mut callback, event_count, _) = build_callback(decoder, |_| true);

        let mut buf = b"not json".to_vec();
        callback(&mut buf);
        // event_count increments on every callback (feed liveness), even when
        // the decoder produces zero hot events.
        assert_eq!(event_count.load(Ordering::Relaxed), 1);
    }
}
