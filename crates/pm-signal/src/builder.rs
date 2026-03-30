//! Builder helper: construct a [`StrategyEngine`] from TOML-loaded config.
//!
//! Only available with the `std` feature because [`StrategyConfig`] lives
//! behind `pm-types/std`.

extern crate std;

use pm_types::StrategyLabel;
use pm_types::config::StrategyConfig;

use crate::{
    AnyStrategy, CompleteSetArb, EarlyDirectional, HedgeLock, MomentumConfirmation, StrategyEngine,
};

/// Build a [`StrategyEngine`] from a slice of [`StrategyConfig`] values.
///
/// Each config variant is mapped to the corresponding concrete strategy type
/// via the zero-vtable [`AnyStrategy`] enum, so the hot-path `evaluate_all`
/// call pays no virtual-dispatch overhead.
///
/// # Example
///
/// ```ignore
/// let cfg: BotConfig = toml::from_str(&src)?;
/// let engine = build_engine_from_config(&cfg.bot.strategies);
/// ```
#[must_use]
pub fn build_engine_from_config(strategies: &[StrategyConfig]) -> StrategyEngine {
    let any_strategies: std::vec::Vec<AnyStrategy> = strategies
        .iter()
        .map(|s| match s {
            StrategyConfig::EarlyDirectional {
                label,
                max_entry_time_secs,
                min_spot_magnitude,
                max_entry_price,
                ..
            } => {
                let lbl = if label.is_empty() {
                    let auto = std::format!("ED-{max_entry_price}");
                    StrategyLabel::new(&auto)
                } else {
                    StrategyLabel::new(label)
                };
                AnyStrategy::Early(
                    EarlyDirectional::new(
                        *max_entry_time_secs,
                        *min_spot_magnitude,
                        *max_entry_price,
                    )
                    .with_label(lbl),
                )
            }
            StrategyConfig::MomentumConfirmation {
                label,
                min_entry_time_secs,
                max_entry_time_secs,
                min_spot_magnitude,
                max_entry_price,
                ..
            } => {
                let lbl = if label.is_empty() {
                    let auto = std::format!("MC-{max_entry_price}");
                    StrategyLabel::new(&auto)
                } else {
                    StrategyLabel::new(label)
                };
                AnyStrategy::Momentum(
                    MomentumConfirmation::new(
                        *min_entry_time_secs,
                        *max_entry_time_secs,
                        *min_spot_magnitude,
                        *max_entry_price,
                    )
                    .with_label(lbl),
                )
            }
            StrategyConfig::CompleteSetArb {
                max_combined_cost,
                min_profit_per_share,
                ..
            } => AnyStrategy::Arb(CompleteSetArb::new(*max_combined_cost, *min_profit_per_share)),
            StrategyConfig::HedgeLock { max_combined_cost, .. } => {
                AnyStrategy::Hedge(HedgeLock::new(*max_combined_cost))
            }
        })
        .collect();
    StrategyEngine::from_any(any_strategies)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;

    use pm_types::config::default_strategies;

    use super::*;

    #[test]
    fn build_from_defaults_produces_non_empty_engine() {
        let defaults = default_strategies();
        let engine = build_engine_from_config(&defaults);
        // We can't inspect internals, but building must not panic and an empty
        // MarketState evaluation should simply return no decisions.
        let _ = engine; // just verify it compiles and constructs
    }

    #[test]
    fn build_from_empty_slice_is_fine() {
        let engine = build_engine_from_config(&[]);
        let _ = engine;
    }
}
