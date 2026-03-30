//! Builder helpers: construct a [`StrategyEngine`] or a vec of
//! [`ConcreteStrategyInstance`]s from TOML-loaded config.
//!
//! Only available with the `std` feature because [`StrategyConfig`] lives
//! behind `pm-types/std`.

extern crate std;

use pm_types::StrategyLabel;
use pm_types::config::StrategyConfig;

use crate::{
    AnyStrategy, CompleteSetArb, ConcreteStrategyInstance, EarlyDirectional, HedgeLock,
    LateWindowSniper, MeanReversion, MomentumConfirmation, StrategyEngine,
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
            StrategyConfig::LateWindowSniper {
                label,
                max_remaining_secs,
                min_spot_magnitude,
                max_entry_price,
                ..
            } => {
                let lbl = if label.is_empty() {
                    let auto = std::format!("LWS-{max_entry_price}");
                    StrategyLabel::new(&auto)
                } else {
                    StrategyLabel::new(label)
                };
                AnyStrategy::LateSniper(
                    LateWindowSniper::new(
                        *max_remaining_secs,
                        *min_spot_magnitude,
                        *max_entry_price,
                    )
                    .with_label(lbl),
                )
            }
            StrategyConfig::MeanReversion {
                label,
                min_elapsed_secs,
                min_spot_magnitude,
                max_opposite_price,
                ..
            } => {
                let lbl = if label.is_empty() {
                    let auto = std::format!("MR-{max_opposite_price}");
                    StrategyLabel::new(&auto)
                } else {
                    StrategyLabel::new(label)
                };
                AnyStrategy::MeanRev(
                    MeanReversion::new(
                        *min_elapsed_secs,
                        *min_spot_magnitude,
                        *max_opposite_price,
                    )
                    .with_label(lbl),
                )
            }
        })
        .collect();
    StrategyEngine::from_any(any_strategies)
}

/// Build a vec of independent [`ConcreteStrategyInstance`]s from config.
///
/// Each strategy config becomes a fully independent instance with its own
/// balance, positions, and risk parameters.
#[must_use]
pub fn build_instances_from_config(
    strategies: &[StrategyConfig],
) -> std::vec::Vec<ConcreteStrategyInstance> {
    strategies
        .iter()
        .map(|s| {
            let (label_str, strategy, balance, max_pos, max_exp, kelly, max_loss, slippage) =
                match s {
                    StrategyConfig::EarlyDirectional {
                        label,
                        mode: _,
                        max_entry_time_secs,
                        min_spot_magnitude,
                        max_entry_price,
                        balance,
                        max_position_usdc,
                        max_exposure_usdc,
                        kelly_fraction,
                        max_daily_loss,
                        slippage_bps,
                    } => {
                        let auto_label = if label.is_empty() {
                            std::format!("ED-{max_entry_price}")
                        } else {
                            label.clone()
                        };
                        let strat = AnyStrategy::Early(
                            EarlyDirectional::new(
                                *max_entry_time_secs,
                                *min_spot_magnitude,
                                *max_entry_price,
                            )
                            .with_label(StrategyLabel::new(&auto_label)),
                        );
                        (
                            auto_label,
                            strat,
                            *balance,
                            *max_position_usdc,
                            *max_exposure_usdc,
                            *kelly_fraction,
                            *max_daily_loss,
                            *slippage_bps,
                        )
                    }
                    StrategyConfig::MomentumConfirmation {
                        label,
                        mode: _,
                        min_entry_time_secs,
                        max_entry_time_secs,
                        min_spot_magnitude,
                        max_entry_price,
                        balance,
                        max_position_usdc,
                        max_exposure_usdc,
                        kelly_fraction,
                        max_daily_loss,
                        slippage_bps,
                    } => {
                        let auto_label = if label.is_empty() {
                            std::format!("MC-{max_entry_price}")
                        } else {
                            label.clone()
                        };
                        let strat = AnyStrategy::Momentum(
                            MomentumConfirmation::new(
                                *min_entry_time_secs,
                                *max_entry_time_secs,
                                *min_spot_magnitude,
                                *max_entry_price,
                            )
                            .with_label(StrategyLabel::new(&auto_label)),
                        );
                        (
                            auto_label,
                            strat,
                            *balance,
                            *max_position_usdc,
                            *max_exposure_usdc,
                            *kelly_fraction,
                            *max_daily_loss,
                            *slippage_bps,
                        )
                    }
                    StrategyConfig::CompleteSetArb {
                        mode: _,
                        max_combined_cost,
                        min_profit_per_share,
                        balance,
                        max_position_usdc,
                        max_exposure_usdc,
                        kelly_fraction,
                        max_daily_loss,
                        slippage_bps,
                    } => {
                        let strat = AnyStrategy::Arb(CompleteSetArb::new(
                            *max_combined_cost,
                            *min_profit_per_share,
                        ));
                        (
                            std::string::String::from("CSA"),
                            strat,
                            *balance,
                            *max_position_usdc,
                            *max_exposure_usdc,
                            *kelly_fraction,
                            *max_daily_loss,
                            *slippage_bps,
                        )
                    }
                    StrategyConfig::HedgeLock {
                        mode: _,
                        max_combined_cost,
                        balance,
                        max_position_usdc,
                        max_exposure_usdc,
                        kelly_fraction,
                        max_daily_loss,
                        slippage_bps,
                    } => {
                        let strat = AnyStrategy::Hedge(HedgeLock::new(*max_combined_cost));
                        (
                            std::string::String::from("HL"),
                            strat,
                            *balance,
                            *max_position_usdc,
                            *max_exposure_usdc,
                            *kelly_fraction,
                            *max_daily_loss,
                            *slippage_bps,
                        )
                    }
                    StrategyConfig::LateWindowSniper {
                        label,
                        mode: _,
                        max_remaining_secs,
                        min_spot_magnitude,
                        max_entry_price,
                        balance,
                        max_position_usdc,
                        max_exposure_usdc,
                        kelly_fraction,
                        max_daily_loss,
                        slippage_bps,
                    } => {
                        let auto_label = if label.is_empty() {
                            std::format!("LWS-{max_entry_price}")
                        } else {
                            label.clone()
                        };
                        let strat = AnyStrategy::LateSniper(
                            LateWindowSniper::new(
                                *max_remaining_secs,
                                *min_spot_magnitude,
                                *max_entry_price,
                            )
                            .with_label(StrategyLabel::new(&auto_label)),
                        );
                        (
                            auto_label,
                            strat,
                            *balance,
                            *max_position_usdc,
                            *max_exposure_usdc,
                            *kelly_fraction,
                            *max_daily_loss,
                            *slippage_bps,
                        )
                    }
                    StrategyConfig::MeanReversion {
                        label,
                        mode: _,
                        min_elapsed_secs,
                        min_spot_magnitude,
                        max_opposite_price,
                        balance,
                        max_position_usdc,
                        max_exposure_usdc,
                        kelly_fraction,
                        max_daily_loss,
                        slippage_bps,
                    } => {
                        let auto_label = if label.is_empty() {
                            std::format!("MR-{max_opposite_price}")
                        } else {
                            label.clone()
                        };
                        let strat = AnyStrategy::MeanRev(
                            MeanReversion::new(
                                *min_elapsed_secs,
                                *min_spot_magnitude,
                                *max_opposite_price,
                            )
                            .with_label(StrategyLabel::new(&auto_label)),
                        );
                        (
                            auto_label,
                            strat,
                            *balance,
                            *max_position_usdc,
                            *max_exposure_usdc,
                            *kelly_fraction,
                            *max_daily_loss,
                            *slippage_bps,
                        )
                    }
                };

            ConcreteStrategyInstance::new(
                label_str, strategy, balance, max_pos, max_exp, kelly, max_loss, slippage,
            )
        })
        .collect()
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

    #[test]
    fn build_instances_from_defaults() {
        use pm_types::StrategyInstance;

        let defaults = default_strategies();
        let instances = build_instances_from_config(&defaults);
        assert_eq!(instances.len(), defaults.len());
        for inst in &instances {
            assert!(inst.balance() > 0.0);
            assert!(!inst.label().is_empty());
        }
    }

    #[test]
    fn build_instances_from_empty_slice() {
        let instances = build_instances_from_config(&[]);
        assert!(instances.is_empty());
    }
}
