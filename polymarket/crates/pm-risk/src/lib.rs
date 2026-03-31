//! Risk management: position sizing, exposure limits, and kill switch.

#![deny(unsafe_code)]

use pm_types::{
    Asset, ContractPrice, EntryDecision, OpenPosition, OrderReason, Pnl, Rejection, SizedOrder,
    WindowId,
};
use tracing::{debug, warn};

// ─── RiskConfig ──────────────────────────────────────────────────────────────

/// Risk manager configuration.
pub struct RiskConfig {
    /// Maximum USDC per single position.
    pub max_position_usdc: f64,
    /// Maximum total USDC across all open positions.
    pub max_total_exposure_usdc: f64,
    /// Maximum daily loss before kill switch triggers.
    pub max_daily_loss_usdc: f64,
    /// Fraction of Kelly criterion to use (e.g., 0.25).
    pub kelly_fraction: f64,
    /// Maximum same-side positions before correlation guard triggers (default: 2).
    pub max_same_side_positions: usize,
}

// ─── RiskManager ─────────────────────────────────────────────────────────────

/// The risk manager. Stateful — tracks open positions and daily P&L.
pub struct RiskManager {
    config: RiskConfig,
    open_positions: Vec<OpenPosition>,
    daily_pnl: f64,
    kill_switch_active: bool,
}

impl RiskManager {
    /// Create a new [`RiskManager`] with the given configuration.
    #[must_use]
    pub fn new(config: RiskConfig) -> Self {
        Self {
            config,
            open_positions: Vec::new(),
            daily_pnl: 0.0,
            kill_switch_active: false,
        }
    }

    /// Evaluate an entry decision against all risk rules.
    ///
    /// Returns a [`SizedOrder`] if the trade passes all checks, or a
    /// [`Rejection`] explaining which rule was violated.
    ///
    /// Rules applied in order:
    /// 1. Kill switch (manual or daily-loss-triggered)
    /// 2. Exposure limits (total and per-position cap)
    /// 3. Correlation guard (3+ same-side positions across different assets)
    /// 4. Kelly sizing
    ///
    /// `window_id` and `asset` are required to construct the resulting
    /// [`SizedOrder`] (they are not encoded in [`EntryDecision`]).
    ///
    /// # Errors
    ///
    /// Returns a [`Rejection`] if any risk rule is violated.
    pub fn evaluate(
        &self,
        decision: &EntryDecision,
        window_id: WindowId,
        asset: Asset,
        balance: f64,
    ) -> Result<SizedOrder, Rejection> {
        // ── Rule 1: Kill switch ───────────────────────────────────────────────
        if self.kill_switch_active || self.daily_pnl < -self.config.max_daily_loss_usdc {
            warn!(
                kill_switch = self.kill_switch_active,
                daily_pnl = self.daily_pnl,
                "kill switch active — rejecting trade"
            );
            return Err(Rejection::KillSwitchActive);
        }

        // ── Rule 2: Exposure limits ───────────────────────────────────────────
        let total_exposure = self.total_exposure();

        // Compute the proposed raw Kelly size and cap at max_position_usdc.
        let raw_size = self.kelly_size(decision, balance);
        let proposed = raw_size.min(self.config.max_position_usdc);

        if total_exposure + proposed > self.config.max_total_exposure_usdc {
            warn!(
                total_exposure,
                proposed, "total exposure would exceed limit — rejecting trade"
            );
            return Err(Rejection::TotalExposureLimitBreached);
        }

        // ── Rule 3: Correlation guard ─────────────────────────────────────────
        // Count distinct assets with same-side open positions.
        let same_side_assets: std::collections::HashSet<Asset> = self
            .open_positions
            .iter()
            .filter(|p| p.side == decision.side)
            .map(|p| p.asset)
            .collect();

        if same_side_assets.len() >= self.config.max_same_side_positions {
            warn!(
                side = %decision.side,
                correlated_assets = same_side_assets.len(),
                "correlation guard triggered — too many same-side positions"
            );
            return Err(Rejection::CorrelationGuard);
        }

        // ── Rule 4: Kelly sizing (already computed above) ────────────────────
        let size_usdc = proposed;

        debug!(
            %window_id,
            %asset,
            side = %decision.side,
            size_usdc,
            confidence = decision.confidence,
            "risk check passed — emitting sized order"
        );

        Ok(SizedOrder {
            window_id,
            asset,
            side: decision.side,
            limit_price: decision.limit_price,
            size_usdc,
            reason: OrderReason::NewSignal,
        })
    }

    /// Record a new open position.
    pub fn on_position_opened(&mut self, position: OpenPosition) {
        debug!(
            window_id = %position.window_id,
            asset = %position.asset,
            side = %position.side,
            size_usdc = position.size_usdc,
            "position opened"
        );
        self.open_positions.push(position);
    }

    /// Record a position closed and update daily P&L.
    pub fn on_position_closed(&mut self, window_id: WindowId, pnl: Pnl) {
        let initial_len = self.open_positions.len();
        self.open_positions.retain(|p| p.window_id != window_id);
        let removed = initial_len - self.open_positions.len();

        self.daily_pnl += pnl.as_f64();

        debug!(
            %window_id,
            pnl = pnl.as_f64(),
            daily_pnl = self.daily_pnl,
            positions_removed = removed,
            "position closed"
        );
    }

    /// Remove all positions for a resolved window and update daily P&L.
    pub fn on_window_resolved(&mut self, window_id: WindowId, pnl: Pnl) {
        self.on_position_closed(window_id, pnl);
    }

    /// Check open positions for hedge opportunities.
    ///
    /// For each open position, if `entry_price + opposite_ask < 1.0` the hedge
    /// would lock an arbitrage profit. The caller supplies `get_opposite_ask`
    /// to look up the current ask for the opposite-side contract.
    pub fn check_hedges(
        &self,
        get_opposite_ask: impl Fn(&OpenPosition) -> Option<ContractPrice>,
    ) -> Vec<SizedOrder> {
        let mut hedges = Vec::new();

        for position in &self.open_positions {
            let Some(opp_ask) = get_opposite_ask(position) else {
                continue;
            };

            // Combined cost < 1.0 means buying both sides is cheaper than the
            // guaranteed $1 payout — a hedge locks in the spread.
            let combined_cost = position.avg_entry.as_f64() + opp_ask.as_f64();
            if combined_cost >= 1.0 {
                continue;
            }

            let size_usdc = position.size_usdc.min(self.config.max_position_usdc);

            debug!(
                window_id = %position.window_id,
                asset = %position.asset,
                combined_cost,
                size_usdc,
                "hedge opportunity detected"
            );

            hedges.push(SizedOrder {
                window_id: position.window_id,
                asset: position.asset,
                side: position.side.opposite(),
                limit_price: opp_ask,
                size_usdc,
                reason: OrderReason::EarlyClose,
            });
        }

        hedges
    }

    /// Toggle the kill switch manually.
    pub fn set_kill_switch(&mut self, active: bool) {
        warn!(active, "kill switch set manually");
        self.kill_switch_active = active;
    }

    /// Returns `true` if the kill switch is currently active.
    #[must_use]
    pub fn is_kill_switch_active(&self) -> bool {
        self.kill_switch_active
    }

    /// Current daily P&L in USDC.
    #[must_use]
    pub fn daily_pnl(&self) -> f64 {
        self.daily_pnl
    }

    /// Current total open exposure in USDC.
    #[must_use]
    pub fn total_exposure(&self) -> f64 {
        self.open_positions.iter().map(|p| p.size_usdc).sum()
    }

    /// Number of currently open positions.
    #[must_use]
    pub fn open_position_count(&self) -> usize {
        self.open_positions.len()
    }

    /// Reset the daily P&L counter (called at midnight UTC).
    ///
    /// Does not clear the kill switch — that must be reset explicitly via
    /// [`Self::set_kill_switch`].
    pub fn reset_daily(&mut self) {
        debug!(previous_pnl = self.daily_pnl, "resetting daily P&L");
        self.daily_pnl = 0.0;
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Compute the raw Kelly-fraction–sized position in USDC.
    ///
    /// `size = kelly_fraction × confidence × balance`
    fn kelly_size(&self, decision: &EntryDecision, balance: f64) -> f64 {
        self.config.kelly_fraction * decision.confidence * balance
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;

    use pm_types::{
        Asset, ContractPrice, EntryDecision, OpenPosition, Pnl, Side, StrategyId, StrategyLabel,
        WindowId,
    };

    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn default_config() -> RiskConfig {
        RiskConfig {
            max_position_usdc: 50.0,
            max_total_exposure_usdc: 200.0,
            max_daily_loss_usdc: 100.0,
            kelly_fraction: 0.25,
            max_same_side_positions: 2,
        }
    }

    fn make_decision(side: Side, confidence: f64) -> EntryDecision {
        EntryDecision {
            side,
            limit_price: ContractPrice::new(0.55).expect("valid price"),
            confidence,
            strategy_id: StrategyId::EarlyDirectional,
            label: StrategyLabel::EMPTY,
        }
    }

    fn make_position(window_id: u64, asset: Asset, side: Side, size_usdc: f64) -> OpenPosition {
        OpenPosition {
            window_id: WindowId::new(window_id),
            asset,
            side,
            avg_entry: ContractPrice::new(0.55).expect("valid price"),
            size_usdc,
            opened_at_ms: 0,
        }
    }

    // ── Test 1: Kill switch — manual activation rejects all evaluations ───────

    #[test]
    fn kill_switch_active_rejects_all_evaluations() {
        let mut rm = RiskManager::new(default_config());
        rm.set_kill_switch(true);

        let decision = make_decision(Side::Up, 0.8);
        let result = rm.evaluate(&decision, WindowId::new(1), Asset::Btc, 500.0);
        assert_eq!(result, Err(Rejection::KillSwitchActive));
    }

    // ── Test 2: Daily loss trigger ────────────────────────────────────────────

    #[test]
    fn daily_loss_trigger_rejects_when_limit_breached() {
        let mut rm = RiskManager::new(default_config());

        // Set daily P&L below the -100 threshold.
        rm.daily_pnl = -101.0;

        let decision = make_decision(Side::Up, 0.8);
        let result = rm.evaluate(&decision, WindowId::new(1), Asset::Btc, 500.0);
        assert_eq!(result, Err(Rejection::KillSwitchActive));
    }

    // ── Test 3: Exposure limit ────────────────────────────────────────────────

    #[test]
    fn exposure_limit_rejects_when_total_exceeded() {
        let mut rm = RiskManager::new(default_config());

        // Fill 190 USDC of the 200 USDC limit.
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 95.0));
        rm.on_position_opened(make_position(2, Asset::Eth, Side::Down, 95.0));

        // Kelly: 0.25 * 1.0 * 1000 = 250 → capped at 50. 190 + 50 = 240 > 200.
        let decision = make_decision(Side::Up, 1.0);
        let result = rm.evaluate(&decision, WindowId::new(3), Asset::Sol, 1000.0);
        assert_eq!(result, Err(Rejection::TotalExposureLimitBreached));
    }

    // ── Test 4: Position size cap ─────────────────────────────────────────────

    #[test]
    fn position_size_capped_at_max_position_usdc() {
        let rm = RiskManager::new(default_config());

        // Raw Kelly: 0.25 * 1.0 * 10_000 = 2500 → capped at max_position_usdc=50.
        let decision = make_decision(Side::Up, 1.0);
        let order = rm
            .evaluate(&decision, WindowId::new(1), Asset::Btc, 10_000.0)
            .expect("should pass risk checks");
        assert!((order.size_usdc - 50.0).abs() < f64::EPSILON);
    }

    // ── Test 5: Correlation guard ─────────────────────────────────────────────

    #[test]
    fn correlation_guard_rejects_third_same_side_position() {
        let mut rm = RiskManager::new(default_config());

        // Two Up positions already open on different assets.
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 10.0));
        rm.on_position_opened(make_position(2, Asset::Eth, Side::Up, 10.0));

        // Third Up position should be rejected.
        let decision = make_decision(Side::Up, 0.5);
        let result = rm.evaluate(&decision, WindowId::new(3), Asset::Sol, 500.0);
        assert_eq!(result, Err(Rejection::CorrelationGuard));
    }

    #[test]
    fn correlation_guard_allows_second_same_side_position() {
        let mut rm = RiskManager::new(default_config());

        // Only one existing Up position — a second should be allowed.
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 10.0));

        let decision = make_decision(Side::Up, 0.5);
        let result = rm.evaluate(&decision, WindowId::new(2), Asset::Eth, 500.0);
        assert!(result.is_ok(), "two same-side positions should be allowed");
    }

    // ── Test 6: Kelly sizing scales with confidence ───────────────────────────

    #[test]
    fn kelly_sizing_scales_with_confidence() {
        let rm = RiskManager::new(default_config());
        let balance = 100.0;

        // 0.25 * 0.2 * 100 = 5.0
        let low = make_decision(Side::Up, 0.2);
        let order_low = rm
            .evaluate(&low, WindowId::new(1), Asset::Btc, balance)
            .expect("low confidence should pass");

        // 0.25 * 0.8 * 100 = 20.0
        let high = make_decision(Side::Up, 0.8);
        let order_high = rm
            .evaluate(&high, WindowId::new(2), Asset::Btc, balance)
            .expect("high confidence should pass");

        assert!(
            order_high.size_usdc > order_low.size_usdc,
            "higher confidence should produce larger order"
        );
        assert!((order_low.size_usdc - 5.0).abs() < f64::EPSILON);
        assert!((order_high.size_usdc - 20.0).abs() < f64::EPSILON);
    }

    // ── Test 7: Hedge check ───────────────────────────────────────────────────

    #[test]
    fn hedge_check_generates_order_when_cheap_opposite() {
        let mut rm = RiskManager::new(default_config());

        // Entry at 0.55; opposite is now 0.40. Combined = 0.95 < 1.0.
        let position = make_position(1, Asset::Btc, Side::Up, 20.0);
        rm.on_position_opened(position);

        let hedges = rm.check_hedges(|pos| {
            assert_eq!(pos.window_id, WindowId::new(1));
            ContractPrice::new(0.40)
        });

        assert_eq!(hedges.len(), 1);
        let hedge = hedges[0];
        assert_eq!(hedge.window_id, WindowId::new(1));
        assert_eq!(hedge.asset, Asset::Btc);
        assert_eq!(hedge.side, Side::Down);
        assert!((hedge.limit_price.as_f64() - 0.40).abs() < f64::EPSILON);
    }

    #[test]
    fn hedge_check_no_order_when_combined_cost_at_or_above_one() {
        let mut rm = RiskManager::new(default_config());

        let position = make_position(1, Asset::Btc, Side::Up, 20.0);
        rm.on_position_opened(position);

        // combined = 0.55 + 0.50 = 1.05 — no hedge.
        let hedges = rm.check_hedges(|_| ContractPrice::new(0.50));
        assert!(hedges.is_empty());
    }

    // ── Test 8: on_position_closed updates P&L ────────────────────────────────

    #[test]
    fn on_position_closed_updates_daily_pnl() {
        let mut rm = RiskManager::new(default_config());
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 20.0));

        assert_eq!(rm.open_position_count(), 1);
        assert!((rm.daily_pnl() - 0.0).abs() < f64::EPSILON);

        let pnl = Pnl::new(8.50).expect("valid pnl");
        rm.on_position_closed(WindowId::new(1), pnl);

        assert_eq!(rm.open_position_count(), 0);
        assert!((rm.daily_pnl() - 8.50).abs() < f64::EPSILON);
    }

    #[test]
    fn on_position_closed_accumulates_multiple_pnl_values() {
        let mut rm = RiskManager::new(default_config());
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 10.0));
        rm.on_position_opened(make_position(2, Asset::Eth, Side::Down, 10.0));

        rm.on_position_closed(WindowId::new(1), Pnl::new(5.0).expect("valid"));
        rm.on_position_closed(WindowId::new(2), Pnl::new(-3.0).expect("valid"));

        assert!((rm.daily_pnl() - 2.0).abs() < f64::EPSILON);
    }

    // ── Test 9: reset_daily ───────────────────────────────────────────────────

    #[test]
    fn reset_daily_clears_daily_pnl() {
        let mut rm = RiskManager::new(default_config());
        rm.daily_pnl = -42.0;

        rm.reset_daily();

        assert!((rm.daily_pnl() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_daily_does_not_clear_kill_switch() {
        let mut rm = RiskManager::new(default_config());
        rm.set_kill_switch(true);
        rm.daily_pnl = -42.0;

        rm.reset_daily();

        assert!(rm.is_kill_switch_active());
        assert!((rm.daily_pnl() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_daily_allows_trading_after_loss_limit_cleared() {
        let mut rm = RiskManager::new(default_config());
        rm.daily_pnl = -150.0;

        let decision = make_decision(Side::Up, 0.5);
        assert_eq!(
            rm.evaluate(&decision, WindowId::new(1), Asset::Btc, 500.0),
            Err(Rejection::KillSwitchActive)
        );

        rm.reset_daily();
        let result = rm.evaluate(&decision, WindowId::new(1), Asset::Btc, 500.0);
        assert!(result.is_ok(), "should be allowed after daily reset");
    }

    // ── Additional coverage ───────────────────────────────────────────────────

    #[test]
    fn total_exposure_sums_all_open_positions() {
        let mut rm = RiskManager::new(default_config());
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 30.0));
        rm.on_position_opened(make_position(2, Asset::Eth, Side::Down, 20.0));

        assert!((rm.total_exposure() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn on_window_resolved_removes_position_and_updates_pnl() {
        let mut rm = RiskManager::new(default_config());
        rm.on_position_opened(make_position(1, Asset::Btc, Side::Up, 25.0));

        rm.on_window_resolved(WindowId::new(1), Pnl::new(-5.0).expect("valid"));

        assert_eq!(rm.open_position_count(), 0);
        assert!((rm.daily_pnl() - (-5.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn is_kill_switch_active_reflects_state() {
        let mut rm = RiskManager::new(default_config());
        assert!(!rm.is_kill_switch_active());

        rm.set_kill_switch(true);
        assert!(rm.is_kill_switch_active());

        rm.set_kill_switch(false);
        assert!(!rm.is_kill_switch_active());
    }
}
