//! Core trait for fair value estimation.

use pm_types::{Asset, ContractPrice, Timeframe};

/// Estimates the fair value of an Up contract given market state.
///
/// Implementations must be pure: same inputs produce same output, no I/O.
pub trait FairValueEstimator {
    /// Estimate probability that the window resolves Up.
    ///
    /// - `magnitude`: `abs(current - open) / open` as a fraction (e.g., `0.003` = 0.3%)
    /// - `time_remaining_secs`: seconds until the window closes
    fn estimate(
        &self,
        magnitude: f64,
        time_remaining_secs: u64,
        asset: Asset,
        timeframe: Timeframe,
    ) -> ContractPrice;
}
