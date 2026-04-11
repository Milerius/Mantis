//! Feed liveness monitor.
//!
//! Tracks event emission counters from feed spawn wrappers and detects
//! staleness. The engine calls [`FeedMonitor::check_all`] on each
//! `Timer(Periodic)` event.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mantis_types::SourceId;

/// Maximum number of feeds that can be monitored.
pub const MAX_FEEDS: usize = 8;

/// Feed liveness monitor.
///
/// Passively tracks event emission counters and detects feeds that have
/// stopped producing events since the last check cycle.
pub struct FeedMonitor {
    feeds: [Option<FeedState>; MAX_FEEDS],
    len: usize,
}

struct FeedState {
    event_count: Arc<AtomicU64>,
    last_seen: u64,
    source_id: SourceId,
    stale: bool,
    first_check: bool,
}

/// Information about a stale feed.
#[derive(Debug, Clone, Copy)]
pub struct StaleFeedInfo {
    /// Source identifier of the stale feed.
    pub source_id: SourceId,
    /// Last observed event count.
    pub last_event_count: u64,
}

/// Error when registering more feeds than [`MAX_FEEDS`].
#[derive(Debug)]
pub struct MonitorFullError;

impl std::fmt::Display for MonitorFullError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("feed monitor is full")
    }
}

impl std::error::Error for MonitorFullError {}

impl FeedMonitor {
    /// Creates a new empty feed monitor.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            feeds: [None, None, None, None, None, None, None, None],
            len: 0,
        }
    }

    /// Registers a feed for monitoring.
    ///
    /// The `event_count` should be the shared atomic counter from the feed
    /// spawn result. The first [`check_all`](Self::check_all) call after
    /// registration establishes a baseline and will not mark the feed stale.
    ///
    /// # Errors
    ///
    /// Returns [`MonitorFullError`] if [`MAX_FEEDS`] feeds are already registered.
    pub fn register(
        &mut self,
        source_id: SourceId,
        event_count: Arc<AtomicU64>,
    ) -> Result<(), MonitorFullError> {
        if self.len >= MAX_FEEDS {
            return Err(MonitorFullError);
        }
        let current = event_count.load(Ordering::Relaxed);
        self.feeds[self.len] = Some(FeedState {
            event_count,
            last_seen: current,
            source_id,
            stale: false,
            first_check: true,
        });
        self.len += 1;
        Ok(())
    }

    /// Checks all registered feeds for staleness.
    ///
    /// For each feed, compares the current event count against the last
    /// observed value. On the first check after registration, this
    /// establishes a baseline without marking the feed stale.
    ///
    /// Returns the number of currently stale feeds.
    pub fn check_all(&mut self) -> usize {
        let mut stale_count = 0;
        for slot in &mut self.feeds[..self.len] {
            let Some(feed) = slot.as_mut() else {
                continue;
            };
            let current = feed.event_count.load(Ordering::Relaxed);
            if feed.first_check {
                feed.first_check = false;
                feed.last_seen = current;
                continue;
            }
            if current == feed.last_seen {
                feed.stale = true;
                stale_count += 1;
            } else {
                feed.stale = false;
            }
            feed.last_seen = current;
        }
        stale_count
    }

    /// Returns an iterator over currently stale feeds.
    pub fn stale_feeds(&self) -> impl Iterator<Item = StaleFeedInfo> + '_ {
        self.feeds[..self.len].iter().filter_map(|slot| {
            let feed = slot.as_ref()?;
            if feed.stale {
                Some(StaleFeedInfo {
                    source_id: feed.source_id,
                    last_event_count: feed.last_seen,
                })
            } else {
                None
            }
        })
    }

    /// Checks whether a specific feed is currently marked stale.
    #[must_use]
    pub fn is_stale(&self, source_id: SourceId) -> bool {
        self.feeds[..self.len].iter().any(|slot| {
            slot.as_ref()
                .is_some_and(|f| f.source_id == source_id && f.stale)
        })
    }

    /// Returns the number of registered feeds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if no feeds are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for FeedMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(val: u64) -> Arc<AtomicU64> {
        Arc::new(AtomicU64::new(val))
    }

    #[test]
    fn new_monitor_is_empty() {
        let mon = FeedMonitor::new();
        assert_eq!(mon.len(), 0);
        assert!(mon.is_empty());
    }

    #[test]
    fn first_check_establishes_baseline() {
        let cnt = counter(0);
        let mut mon = FeedMonitor::new();
        mon.register(SourceId::from_raw(1), Arc::clone(&cnt)).ok();
        let stale = mon.check_all();
        assert_eq!(stale, 0);
        assert!(!mon.is_stale(SourceId::from_raw(1)));
    }

    #[test]
    fn unchanged_after_baseline_is_stale() {
        let cnt = counter(0);
        let mut mon = FeedMonitor::new();
        mon.register(SourceId::from_raw(1), Arc::clone(&cnt)).ok();
        mon.check_all(); // baseline
        let stale = mon.check_all(); // no change
        assert_eq!(stale, 1);
        assert!(mon.is_stale(SourceId::from_raw(1)));
    }

    #[test]
    fn active_feed_not_stale() {
        let cnt = counter(0);
        let mut mon = FeedMonitor::new();
        mon.register(SourceId::from_raw(1), Arc::clone(&cnt)).ok();
        mon.check_all(); // baseline
        cnt.fetch_add(5, Ordering::Relaxed);
        let stale = mon.check_all();
        assert_eq!(stale, 0);
        assert!(!mon.is_stale(SourceId::from_raw(1)));
    }

    #[test]
    fn recovery_clears_stale() {
        let cnt = counter(0);
        let mut mon = FeedMonitor::new();
        mon.register(SourceId::from_raw(1), Arc::clone(&cnt)).ok();
        mon.check_all(); // baseline
        assert_eq!(mon.check_all(), 1); // stale
        assert!(mon.is_stale(SourceId::from_raw(1)));
        cnt.fetch_add(1, Ordering::Relaxed);
        assert_eq!(mon.check_all(), 0); // recovered
        assert!(!mon.is_stale(SourceId::from_raw(1)));
    }

    #[test]
    fn multiple_feeds() {
        let cnt_a = counter(0);
        let cnt_b = counter(0);
        let mut mon = FeedMonitor::new();
        mon.register(SourceId::from_raw(1), Arc::clone(&cnt_a)).ok();
        mon.register(SourceId::from_raw(2), Arc::clone(&cnt_b)).ok();
        mon.check_all(); // baseline for both
        cnt_a.fetch_add(1, Ordering::Relaxed);
        // cnt_b unchanged
        let stale = mon.check_all();
        assert_eq!(stale, 1);
        assert!(!mon.is_stale(SourceId::from_raw(1)));
        assert!(mon.is_stale(SourceId::from_raw(2)));
    }

    #[test]
    fn max_feeds_exceeded() {
        let mut mon = FeedMonitor::new();
        for i in 0..MAX_FEEDS {
            #[expect(clippy::cast_possible_truncation)]
            let id = SourceId::from_raw(i as u16);
            assert!(mon.register(id, counter(0)).is_ok());
        }
        #[expect(clippy::cast_possible_truncation)]
        let overflow_id = SourceId::from_raw(MAX_FEEDS as u16);
        assert!(mon.register(overflow_id, counter(0)).is_err());
    }

    #[test]
    fn stale_feeds_iterator() {
        let cnt_a = counter(0);
        let cnt_b = counter(0);
        let mut mon = FeedMonitor::new();
        mon.register(SourceId::from_raw(1), Arc::clone(&cnt_a)).ok();
        mon.register(SourceId::from_raw(2), Arc::clone(&cnt_b)).ok();
        mon.check_all(); // baseline
        // both unchanged
        mon.check_all();
        let stale: Vec<_> = mon.stale_feeds().collect();
        assert_eq!(stale.len(), 2);
    }
}
