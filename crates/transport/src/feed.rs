//! Generic feed thread — a pinned blocking thread running a WS read loop.
//!
//! Each feed (Polymarket market, Polymarket user, Binance reference) runs as
//! a `FeedThread`. The thread connects to the WebSocket endpoint, reads text
//! frames in a blocking loop, and calls a user-provided callback for each
//! message. Reconnection with exponential backoff is handled automatically.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

use crate::tuning::SocketTuning;
use crate::ws::{WsConfig, WsConnection, WsError};

/// Configuration for a feed thread.
#[derive(Clone, Debug)]
pub struct FeedConfig {
    /// Human-readable name for this feed (used in thread name and logs).
    pub name: String,
    /// WebSocket connection config.
    pub ws: WsConfig,
    /// Socket and CPU tuning.
    pub tuning: SocketTuning,
    /// Reconnection backoff config.
    pub backoff: BackoffConfig,
}

/// Exponential backoff parameters for reconnection.
#[derive(Clone, Debug)]
pub struct BackoffConfig {
    /// Initial delay after first disconnect.
    pub initial: Duration,
    /// Maximum delay between reconnection attempts.
    pub max: Duration,
    /// Jitter factor (0.0 = no jitter, 0.125 = ±12.5%).
    pub jitter_factor: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
            jitter_factor: 0.125,
        }
    }
}

impl BackoffConfig {
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn delay(&self, attempt: u32) -> Duration {
        let base = self
            .initial
            .saturating_mul(1u32.wrapping_shl(attempt.min(20)));
        let capped = base.min(self.max);

        if self.jitter_factor <= 0.0 {
            return capped;
        }

        let nanos = Instant::now().elapsed().subsec_nanos();
        let jitter_range = capped.as_millis() as f64 * self.jitter_factor;
        let jitter_ms = f64::from(nanos % 1000) / 1000.0 * jitter_range * 2.0 - jitter_range;
        let ms = capped.as_millis() as f64 + jitter_ms;
        Duration::from_millis(ms.max(100.0) as u64)
    }
}

/// Handle to a running feed thread.
pub struct FeedHandle {
    join: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    /// Number of messages dropped due to full queue (readable for monitoring).
    pub drops: Arc<AtomicU64>,
    /// Number of successful reconnections (readable for monitoring).
    pub reconnects: Arc<AtomicU64>,
    /// Number of messages processed (readable for monitoring).
    pub msg_count: Arc<AtomicU64>,
}

impl FeedHandle {
    /// Signal the feed thread to stop and wait for it to exit.
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    /// Check if the feed thread is still running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.join.as_ref().is_some_and(|j| !j.is_finished())
    }
}

impl Drop for FeedHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

/// A feed thread that connects to a WebSocket and processes messages.
///
/// The callback `F` receives each text message as a `&str`. It is responsible
/// for parsing, normalizing, and pushing events into the SPSC queue.
/// The callback returns `true` to continue or `false` to stop the feed.
pub struct FeedThread;

impl FeedThread {
    /// Spawn a feed thread that calls `on_message` for each WS text frame.
    ///
    /// The thread runs until [`FeedHandle::shutdown()`] is called or the
    /// callback returns `false`.
    ///
    /// Spawn a feed thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the OS fails to spawn the thread.
    pub fn spawn<F>(config: FeedConfig, on_message: F) -> Result<FeedHandle, std::io::Error>
    where
        F: FnMut(&str) -> bool + Send + 'static,
    {
        let shutdown = Arc::new(AtomicBool::new(false));
        let drops = Arc::new(AtomicU64::new(0));
        let reconnects = Arc::new(AtomicU64::new(0));
        let msg_count = Arc::new(AtomicU64::new(0));

        let shutdown_flag = Arc::clone(&shutdown);
        let reconnects_counter = Arc::clone(&reconnects);
        let msg_counter = Arc::clone(&msg_count);

        let join = thread::Builder::new()
            .name(config.name.clone())
            .spawn(move || {
                feed_loop(
                    &config,
                    on_message,
                    &shutdown_flag,
                    &reconnects_counter,
                    &msg_counter,
                );
            })?;

        Ok(FeedHandle {
            join: Some(join),
            shutdown,
            drops,
            reconnects,
            msg_count,
        })
    }
}

fn feed_loop<F>(
    config: &FeedConfig,
    mut on_message: F,
    shutdown: &Arc<AtomicBool>,
    reconnects: &Arc<AtomicU64>,
    msg_count: &Arc<AtomicU64>,
) where
    F: FnMut(&str) -> bool + Send + 'static,
{
    config.tuning.apply_affinity();

    let mut attempt: u32 = 0;

    while !shutdown.load(Ordering::Acquire) {
        let mut conn = match WsConnection::connect(&config.ws) {
            Ok(conn) => {
                if attempt > 0 {
                    reconnects.fetch_add(1, Ordering::Relaxed);
                    info!(
                        feed = %config.name,
                        attempts = attempt,
                        "reconnected"
                    );
                }
                attempt = 0;
                conn
            }
            Err(e) => {
                let delay = config.backoff.delay(attempt);
                #[expect(clippy::cast_possible_truncation)]
                let delay_ms = delay.as_millis() as u64;
                warn!(
                    feed = %config.name,
                    attempt,
                    delay_ms,
                    error = %e,
                    "connection failed, backing off"
                );
                attempt = attempt.saturating_add(1);
                thread::sleep(delay);
                continue;
            }
        };

        loop {
            if shutdown.load(Ordering::Acquire) {
                info!(feed = %config.name, "shutdown requested");
                return;
            }

            match conn.read_text() {
                Ok(Some(text)) => {
                    msg_count.fetch_add(1, Ordering::Relaxed);
                    if !on_message(&text) {
                        info!(feed = %config.name, "callback requested stop");
                        return;
                    }
                }
                Ok(None) => {}
                Err(WsError::Closed) => {
                    warn!(feed = %config.name, "server closed connection");
                    break;
                }
                Err(e) => {
                    error!(
                        feed = %config.name,
                        error = %e,
                        "read error, reconnecting"
                    );
                    break;
                }
            }
        }

        let delay = config.backoff.delay(attempt);
        #[expect(clippy::cast_possible_truncation)]
        let delay_ms = delay.as_millis() as u64;
        warn!(
            feed = %config.name,
            delay_ms,
            "reconnecting after delay"
        );
        attempt = attempt.saturating_add(1);
        thread::sleep(delay);
    }

    info!(feed = %config.name, "feed thread exiting");
}
