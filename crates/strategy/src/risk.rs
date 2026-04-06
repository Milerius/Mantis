//! Per-strategy risk configuration.

use mantis_fixed::FixedI64;
use mantis_types::Lots;

/// Per-strategy risk limits. Checked by the Risk Gate on the engine thread.
#[derive(Clone, Debug)]
pub struct RiskLimits {
    /// Max position per instrument (absolute value).
    pub max_position_per_instrument: Lots,
    /// Max total capital deployed across all instruments.
    pub max_capital_deployed: FixedI64<6>,
    /// Max worst-case loss (filled + open orders could fill at expiry).
    pub max_worst_case_loss: FixedI64<6>,
    /// Max resting orders on book.
    pub max_orders_live: u16,
    /// Max intents emitted per second (runaway brake).
    pub max_intents_per_sec: u16,
    /// Max cancel/replace per minute (churn limit).
    pub max_replaces_per_min: u16,
    /// Capital budget allocated to this strategy.
    pub capital_budget: FixedI64<6>,
}

/// Result of a risk check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskCheckResult {
    /// Intent is allowed.
    Pass,
    /// Position limit would be exceeded.
    RejectPosition,
    /// Capital budget would be exceeded.
    RejectCapital,
    /// Too many live orders.
    RejectOrderCount,
    /// Intent rate exceeded.
    RejectRate,
    /// Market data feed is unhealthy.
    RejectFeedUnhealthy,
}
