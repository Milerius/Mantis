//! Smart entry timing: delays execution after a signal fires until optimal
//! conditions are met (spread improvement, better ask) or a timeout expires.

use pm_types::EntryDecision;

// ─── EntryTimer ─────────────────────────────────────────────────────────────

/// Tracks pending signals and decides when to actually execute.
///
/// After a signal fires, monitors conditions for up to `max_wait_secs`
/// before either executing or expiring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryTimer {
    /// Maximum seconds to wait for optimal conditions after signal fires.
    pub max_wait_secs: u64,
    /// Minimum spread improvement (% narrower than at signal time) to trigger early entry.
    pub min_spread_improvement: f64,
}

/// A pending entry that is waiting for optimal execution conditions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingEntry {
    /// The entry decision from the strategy engine.
    pub decision: EntryDecision,
    /// Unix timestamp in milliseconds when the signal originally fired.
    pub signal_time_ms: u64,
    /// Spread at the time the signal fired (`None` if unavailable).
    pub initial_spread: Option<f64>,
    /// Best (lowest) ask price observed since the signal fired.
    pub best_price_seen: f64,
}

impl EntryTimer {
    /// Create a new entry timer with the given parameters.
    #[must_use]
    pub fn new(max_wait_secs: u64, min_spread_improvement: f64) -> Self {
        Self {
            max_wait_secs,
            min_spread_improvement,
        }
    }

    /// Check if a pending entry should execute now.
    ///
    /// Returns `true` if:
    /// - `max_wait_secs` exceeded (execute at best price seen)
    /// - spread improved by at least `min_spread_improvement` fraction
    /// - ask price dropped below the initial ask (better fill)
    #[must_use]
    pub fn should_execute(
        &self,
        pending: &PendingEntry,
        current_time_ms: u64,
        current_ask: Option<f64>,
        current_spread: Option<f64>,
    ) -> bool {
        // Timeout: always execute if max_wait exceeded.
        let elapsed_ms = current_time_ms.saturating_sub(pending.signal_time_ms);
        if elapsed_ms >= self.max_wait_secs * 1_000 {
            return true;
        }

        // Spread improvement check.
        if let (Some(initial), Some(current)) = (pending.initial_spread, current_spread)
            && initial > 0.0
        {
            let improvement = (initial - current) / initial;
            if improvement >= self.min_spread_improvement {
                return true;
            }
        }

        // Better ask price check: if current ask dropped below the initial ask
        // (the limit_price on the decision), we can get a better fill.
        if let Some(ask) = current_ask {
            let initial_ask = pending.decision.limit_price.as_f64();
            if ask < initial_ask {
                return true;
            }
        }

        false
    }

    /// Update the best price tracking for a pending entry.
    ///
    /// Tracks the lowest ask price seen since the signal fired.
    pub fn update_best_price(pending: &mut PendingEntry, current_ask: f64) {
        if current_ask < pending.best_price_seen {
            pending.best_price_seen = current_ask;
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use pm_types::{ContractPrice, Side, StrategyId};

    use super::*;

    fn make_decision(limit_price: f64) -> EntryDecision {
        EntryDecision {
            side: Side::Up,
            limit_price: ContractPrice::new(limit_price).expect("valid"),
            confidence: 0.8,
            strategy_id: StrategyId::EarlyDirectional,
        }
    }

    fn make_pending(limit_price: f64, signal_time_ms: u64, initial_spread: Option<f64>) -> PendingEntry {
        PendingEntry {
            decision: make_decision(limit_price),
            signal_time_ms,
            initial_spread,
            best_price_seen: limit_price,
        }
    }

    #[test]
    fn immediate_execution_when_max_wait_zero() {
        let timer = EntryTimer::new(0, 0.02);
        let pending = make_pending(0.55, 1_000_000, Some(0.04));

        // Any time at or after signal time should trigger.
        assert!(timer.should_execute(&pending, 1_000_000, Some(0.55), Some(0.04)));
    }

    #[test]
    fn executes_when_spread_improves() {
        let timer = EntryTimer::new(10, 0.02); // 2% spread improvement required
        let pending = make_pending(0.55, 1_000_000, Some(0.04)); // initial spread 4 cents

        // Spread narrowed from 0.04 to 0.038 → improvement = (0.04-0.038)/0.04 = 0.05 = 5%
        assert!(timer.should_execute(&pending, 1_002_000, Some(0.55), Some(0.038)));

        // Spread only narrowed to 0.0396 → improvement = 1% < 2%
        assert!(!timer.should_execute(&pending, 1_002_000, Some(0.55), Some(0.0396)));
    }

    #[test]
    fn executes_after_max_wait_secs() {
        let timer = EntryTimer::new(5, 0.02);
        let pending = make_pending(0.55, 1_000_000, Some(0.04));

        // 4 seconds elapsed — should not execute yet.
        assert!(!timer.should_execute(&pending, 1_004_000, Some(0.55), Some(0.04)));

        // 5 seconds elapsed — timeout triggers.
        assert!(timer.should_execute(&pending, 1_005_000, Some(0.55), Some(0.04)));

        // 6 seconds elapsed — still executes (past timeout).
        assert!(timer.should_execute(&pending, 1_006_000, Some(0.55), Some(0.04)));
    }

    #[test]
    fn executes_when_ask_drops_below_initial() {
        let timer = EntryTimer::new(10, 0.02);
        let pending = make_pending(0.55, 1_000_000, Some(0.04));

        // Ask dropped from 0.55 to 0.54 → better fill available.
        assert!(timer.should_execute(&pending, 1_002_000, Some(0.54), Some(0.04)));

        // Ask stayed at 0.55 → no improvement.
        assert!(!timer.should_execute(&pending, 1_002_000, Some(0.55), Some(0.04)));
    }

    #[test]
    fn best_price_tracking_updates_correctly() {
        let mut pending = make_pending(0.55, 1_000_000, Some(0.04));
        assert!((pending.best_price_seen - 0.55).abs() < 1e-10);

        // Lower price should update.
        EntryTimer::update_best_price(&mut pending, 0.53);
        assert!((pending.best_price_seen - 0.53).abs() < 1e-10);

        // Higher price should NOT update.
        EntryTimer::update_best_price(&mut pending, 0.56);
        assert!((pending.best_price_seen - 0.53).abs() < 1e-10);

        // Even lower price should update.
        EntryTimer::update_best_price(&mut pending, 0.51);
        assert!((pending.best_price_seen - 0.51).abs() < 1e-10);
    }

    #[test]
    fn no_spread_data_does_not_trigger_spread_check() {
        let timer = EntryTimer::new(10, 0.02);
        let pending = make_pending(0.55, 1_000_000, None);

        // No spread data — should not trigger on spread improvement.
        // Ask is same as initial — should not trigger on ask check.
        // Time not expired — should not trigger on timeout.
        assert!(!timer.should_execute(&pending, 1_002_000, Some(0.55), None));
    }

    #[test]
    fn no_ask_data_does_not_trigger_ask_check() {
        let timer = EntryTimer::new(10, 0.02);
        let pending = make_pending(0.55, 1_000_000, Some(0.04));

        // No current ask data — only spread and timeout checks apply.
        assert!(!timer.should_execute(&pending, 1_002_000, None, Some(0.04)));
    }
}
