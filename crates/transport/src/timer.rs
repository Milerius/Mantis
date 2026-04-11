//! Timer thread — periodic tick and heartbeat event emitter.
//!
//! The timer thread is a simple metronome: it sleeps for a configurable
//! interval, then pushes a [`HotEvent::timer`] with [`TimerKind::Periodic`].
//! Every N ticks it also emits a [`HotEvent::heartbeat`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use mantis_events::{EventFlags, HeartbeatPayload, HotEvent, TimerKind, TimerPayload};
use mantis_types::{InstrumentId, SeqNum, SourceId, Timestamp};

use crate::feed::interruptible_sleep;
use crate::tuning::SocketTuning;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the timer thread.
#[derive(Clone, Debug)]
pub struct TimerConfig {
    /// Human-readable thread name.
    pub name: String,
    /// Interval between periodic timer events.
    pub tick_interval: Duration,
    /// Interval between heartbeat events (must be a multiple of `tick_interval`).
    pub heartbeat_interval: Duration,
    /// Source ID stamped on every emitted event.
    pub source_id: SourceId,
    /// Optional CPU core to pin the thread to.
    pub core_id: Option<usize>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from validating a [`TimerConfig`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimerConfigError {
    /// `tick_interval` was zero.
    ZeroTickInterval,
    /// `heartbeat_interval` is shorter than `tick_interval`.
    HeartbeatShorterThanTick,
    /// `heartbeat_interval` is not evenly divisible by `tick_interval`.
    HeartbeatNotDivisible,
    /// `heartbeat_interval / tick_interval` exceeds `u64::MAX`.
    HeartbeatTooLarge,
}

impl core::fmt::Display for TimerConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroTickInterval => write!(f, "tick_interval must be > 0"),
            Self::HeartbeatShorterThanTick => {
                write!(f, "heartbeat_interval must be >= tick_interval")
            }
            Self::HeartbeatNotDivisible => {
                write!(
                    f,
                    "heartbeat_interval must be evenly divisible by tick_interval"
                )
            }
            Self::HeartbeatTooLarge => {
                write!(f, "heartbeat_interval / tick_interval exceeds u64::MAX")
            }
        }
    }
}

impl std::error::Error for TimerConfigError {}

/// Error returned when spawning the timer thread fails.
#[derive(Debug)]
pub enum TimerSpawnError {
    /// The timer configuration is invalid.
    Config(TimerConfigError),
    /// The OS refused to spawn the thread.
    Spawn(std::io::Error),
}

impl core::fmt::Display for TimerSpawnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Config(e) => write!(f, "timer config error: {e}"),
            Self::Spawn(e) => write!(f, "timer spawn error: {e}"),
        }
    }
}

impl std::error::Error for TimerSpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(e) => Some(e),
            Self::Spawn(e) => Some(e),
        }
    }
}

impl From<TimerConfigError> for TimerSpawnError {
    fn from(e: TimerConfigError) -> Self {
        Self::Config(e)
    }
}

impl From<std::io::Error> for TimerSpawnError {
    fn from(e: std::io::Error) -> Self {
        Self::Spawn(e)
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate(config: &TimerConfig) -> Result<u64, TimerConfigError> {
    if config.tick_interval.is_zero() {
        return Err(TimerConfigError::ZeroTickInterval);
    }
    let tick_ns = config.tick_interval.as_nanos();
    let hb_ns = config.heartbeat_interval.as_nanos();
    if hb_ns < tick_ns {
        return Err(TimerConfigError::HeartbeatShorterThanTick);
    }
    if !hb_ns.is_multiple_of(tick_ns) {
        return Err(TimerConfigError::HeartbeatNotDivisible);
    }
    let ratio = hb_ns / tick_ns;
    if ratio > u128::from(u64::MAX) {
        return Err(TimerConfigError::HeartbeatTooLarge);
    }
    #[expect(clippy::cast_possible_truncation, reason = "checked above")]
    let heartbeat_every = ratio as u64;
    Ok(heartbeat_every)
}

// ---------------------------------------------------------------------------
// Timer thread handle
// ---------------------------------------------------------------------------

/// Handle to a running timer thread.
///
/// Dropping the handle signals shutdown (but does not join the thread).
/// Call [`TimerThread::shutdown`] for a graceful stop that waits for exit.
pub struct TimerThread {
    join: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl TimerThread {
    /// Spawn the timer thread.
    ///
    /// The `push` callback is invoked on the timer thread for every emitted
    /// [`HotEvent`] (both ticks and heartbeats).
    ///
    /// # Errors
    ///
    /// Returns [`TimerSpawnError::Config`] if the configuration is invalid, or
    /// [`TimerSpawnError::Spawn`] if the OS refuses to create the thread.
    pub fn spawn<F>(config: TimerConfig, push: F) -> Result<Self, TimerSpawnError>
    where
        F: FnMut(HotEvent) + Send + 'static,
    {
        let heartbeat_every = validate(&config)?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);

        let join = thread::Builder::new()
            .name(config.name.clone())
            .spawn(move || {
                timer_loop(&config, heartbeat_every, push, &shutdown_flag);
            })?;

        Ok(Self {
            join: Some(join),
            shutdown,
        })
    }

    /// Signal the timer thread to stop and wait for it to exit.
    pub fn shutdown(mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for TimerThread {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn timer_loop<F>(config: &TimerConfig, heartbeat_every: u64, mut push: F, shutdown: &AtomicBool)
where
    F: FnMut(HotEvent) + Send + 'static,
{
    let tuning = SocketTuning {
        core_id: config.core_id,
        #[cfg(feature = "tuning")]
        busy_poll_us: None,
    };
    tuning.apply_affinity();

    let mut tick_counter: u64 = 0;
    let mut heartbeat_counter: u32 = 0;
    let mut seq: u64 = 0;

    loop {
        if interruptible_sleep(config.tick_interval, shutdown) {
            return;
        }
        if shutdown.load(Ordering::Acquire) {
            return;
        }

        tick_counter += 1;
        seq += 1;

        let timer_payload = TimerPayload {
            timer_id: 0,
            kind: TimerKind::Periodic,
            _pad: [0; 3],
        };

        push(HotEvent::timer(
            Timestamp::now(),
            SeqNum::from_raw(seq),
            InstrumentId::NONE,
            config.source_id,
            EventFlags::EMPTY,
            timer_payload,
        ));

        if tick_counter.is_multiple_of(heartbeat_every) {
            heartbeat_counter = heartbeat_counter.wrapping_add(1);
            seq += 1;

            push(HotEvent::heartbeat(
                Timestamp::now(),
                SeqNum::from_raw(seq),
                config.source_id,
                EventFlags::EMPTY,
                HeartbeatPayload {
                    counter: heartbeat_counter,
                },
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mantis_events::EventBody;
    use std::sync::Mutex;

    fn test_source() -> SourceId {
        SourceId::from_raw(99)
    }

    #[test]
    #[expect(clippy::expect_used)]
    fn emits_periodic_timer_events() {
        let events: Arc<Mutex<Vec<HotEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_push = Arc::clone(&events);

        let timer = TimerThread::spawn(
            TimerConfig {
                name: "test-timer".into(),
                tick_interval: Duration::from_millis(50),
                heartbeat_interval: Duration::from_millis(200),
                source_id: test_source(),
                core_id: None,
            },
            move |ev| {
                events_push.lock().expect("lock").push(ev);
            },
        )
        .expect("spawn");

        // Generous sleep for CI runners
        thread::sleep(Duration::from_millis(1500));
        timer.shutdown();

        let collected = events.lock().expect("lock");
        let timer_count = collected
            .iter()
            .filter(|e| matches!(e.body, EventBody::Timer(_)))
            .count();
        assert!(
            timer_count >= 2,
            "expected >= 2 timer events, got {timer_count}"
        );
    }

    #[test]
    #[expect(clippy::expect_used)]
    fn emits_heartbeat_events() {
        let events: Arc<Mutex<Vec<HotEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_push = Arc::clone(&events);

        let timer = TimerThread::spawn(
            TimerConfig {
                name: "test-hb".into(),
                tick_interval: Duration::from_millis(50),
                heartbeat_interval: Duration::from_millis(200),
                source_id: test_source(),
                core_id: None,
            },
            move |ev| {
                events_push.lock().expect("lock").push(ev);
            },
        )
        .expect("spawn");

        // Give generous time for CI runners (slow VMs)
        thread::sleep(Duration::from_millis(1500));
        timer.shutdown();

        let collected = events.lock().expect("lock");
        let hb_count = collected
            .iter()
            .filter(|e| matches!(e.body, EventBody::Heartbeat(_)))
            .count();
        assert!(
            hb_count >= 1,
            "expected >= 1 heartbeat events, got {hb_count}"
        );
    }

    #[test]
    #[expect(clippy::expect_used)]
    fn seq_is_monotonic() {
        let events: Arc<Mutex<Vec<HotEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_push = Arc::clone(&events);

        let timer = TimerThread::spawn(
            TimerConfig {
                name: "test-seq".into(),
                tick_interval: Duration::from_millis(50),
                heartbeat_interval: Duration::from_millis(100),
                source_id: test_source(),
                core_id: None,
            },
            move |ev| {
                events_push.lock().expect("lock").push(ev);
            },
        )
        .expect("spawn");

        thread::sleep(Duration::from_secs(1));
        timer.shutdown();

        let collected = events.lock().expect("lock");
        assert!(!collected.is_empty(), "expected some events");

        for window in collected.windows(2) {
            let a = window[0].header.seq;
            let b = window[1].header.seq;
            assert!(b > a, "seq not strictly increasing: {a:?} -> {b:?}");
        }
    }

    #[test]
    #[expect(clippy::expect_used)]
    fn shutdown_exits_quickly() {
        let timer = TimerThread::spawn(
            TimerConfig {
                name: "test-shutdown".into(),
                tick_interval: Duration::from_millis(100),
                heartbeat_interval: Duration::from_millis(100),
                source_id: test_source(),
                core_id: None,
            },
            |_| {},
        )
        .expect("spawn");

        let start = std::time::Instant::now();
        timer.shutdown();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(500),
            "shutdown took {elapsed:?}, expected < 500ms"
        );
    }

    #[test]
    fn zero_tick_interval_errors() {
        let result = TimerThread::spawn(
            TimerConfig {
                name: "bad".into(),
                tick_interval: Duration::ZERO,
                heartbeat_interval: Duration::from_millis(100),
                source_id: test_source(),
                core_id: None,
            },
            |_| {},
        );
        assert!(matches!(
            result,
            Err(TimerSpawnError::Config(TimerConfigError::ZeroTickInterval))
        ));
    }

    #[test]
    fn heartbeat_shorter_than_tick_errors() {
        let result = TimerThread::spawn(
            TimerConfig {
                name: "bad".into(),
                tick_interval: Duration::from_millis(100),
                heartbeat_interval: Duration::from_millis(50),
                source_id: test_source(),
                core_id: None,
            },
            |_| {},
        );
        assert!(matches!(
            result,
            Err(TimerSpawnError::Config(
                TimerConfigError::HeartbeatShorterThanTick
            ))
        ));
    }

    #[test]
    fn heartbeat_not_divisible_errors() {
        let result = TimerThread::spawn(
            TimerConfig {
                name: "bad".into(),
                tick_interval: Duration::from_millis(30),
                heartbeat_interval: Duration::from_millis(100),
                source_id: test_source(),
                core_id: None,
            },
            |_| {},
        );
        assert!(matches!(
            result,
            Err(TimerSpawnError::Config(
                TimerConfigError::HeartbeatNotDivisible
            ))
        ));
    }

    #[test]
    fn timer_config_error_display() {
        let e = TimerConfigError::ZeroTickInterval;
        assert!(!e.to_string().is_empty());
        let e = TimerConfigError::HeartbeatShorterThanTick;
        assert!(!e.to_string().is_empty());
        let e = TimerConfigError::HeartbeatNotDivisible;
        assert!(!e.to_string().is_empty());
        let e = TimerConfigError::HeartbeatTooLarge;
        assert!(!e.to_string().is_empty());
    }

    #[test]
    fn timer_config_error_is_std_error() {
        let e: &dyn std::error::Error = &TimerConfigError::ZeroTickInterval;
        assert!(e.source().is_none());
    }

    #[test]
    fn timer_spawn_error_display() {
        let e = TimerSpawnError::Config(TimerConfigError::ZeroTickInterval);
        assert!(!e.to_string().is_empty());
        // Also cover the Spawn variant display
        let io_err = std::io::Error::other("test");
        let e = TimerSpawnError::Spawn(io_err);
        assert!(!e.to_string().is_empty());
    }

    #[test]
    fn timer_spawn_error_source() {
        let e = TimerSpawnError::Config(TimerConfigError::ZeroTickInterval);
        assert!(std::error::Error::source(&e).is_some());

        let io_err = std::io::Error::other("test");
        let e = TimerSpawnError::Spawn(io_err);
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    #[expect(clippy::expect_used)]
    fn timer_drop_signals_shutdown() {
        let config = TimerConfig {
            name: "test-drop".into(),
            tick_interval: Duration::from_millis(50),
            heartbeat_interval: Duration::from_millis(100),
            source_id: test_source(),
            core_id: None,
        };
        let timer = TimerThread::spawn(config, |_| {}).expect("spawn");
        drop(timer);
        // If we get here, Drop didn't hang — test passes
    }

    #[test]
    fn heartbeat_too_large_errors() {
        let config = TimerConfig {
            name: "test-too-large".into(),
            tick_interval: Duration::from_nanos(1),
            heartbeat_interval: Duration::from_secs(u64::MAX),
            source_id: test_source(),
            core_id: None,
        };
        let result = TimerThread::spawn(config, |_| {});
        assert!(result.is_err());
    }
}
