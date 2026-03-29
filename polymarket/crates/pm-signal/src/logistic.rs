//! Logistic regression model for fair value estimation.
//!
//! P(Up) = sigmoid(β₀ + β₁·magnitude + `β₂·time_norm` + `β₃·magnitude·time_norm`)

use pm_types::{Asset, ContractPrice, Timeframe};

use crate::estimator::FairValueEstimator;

// ─── Coefficients ────────────────────────────────────────────────────────────

/// Logistic regression coefficients for a single (asset, timeframe) pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    /// Intercept term β₀.
    pub beta_0: f64,
    /// Coefficient on magnitude β₁.
    pub beta_1: f64,
    /// Coefficient on normalised time β₂.
    pub beta_2: f64,
    /// Coefficient on the interaction term β₃.
    pub beta_3: f64,
}

impl Default for Coefficients {
    /// All-zeros default → sigmoid(0) = 0.5, a flat prior.
    #[inline]
    fn default() -> Self {
        Self { beta_0: 0.0, beta_1: 0.0, beta_2: 0.0, beta_3: 0.0 }
    }
}

// ─── Sigmoid helper ──────────────────────────────────────────────────────────

/// Standard logistic (sigmoid) function: `1 / (1 + exp(-x))`.
///
/// Uses [`libm::exp`] so this function is available in `no_std` contexts.
#[inline]
fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + libm::exp(-x))
}

// ─── LogisticModel ───────────────────────────────────────────────────────────

/// Per-asset, per-timeframe logistic regression model.
///
/// Default coefficients (all zeros) produce P(Up) = 0.5 for every input.
pub struct LogisticModel {
    coeffs: [[Coefficients; Timeframe::COUNT]; Asset::COUNT],
}

impl LogisticModel {
    /// Create a new model with all-zero (flat prior) coefficients.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            coeffs: [[Coefficients::default(); Timeframe::COUNT]; Asset::COUNT],
        }
    }

    /// Replace the coefficients for a specific `(asset, timeframe)` pair.
    #[inline]
    pub fn set_coefficients(&mut self, asset: Asset, timeframe: Timeframe, coeffs: Coefficients) {
        self.coeffs[asset.index()][timeframe.index()] = coeffs;
    }

    /// Retrieve the coefficients for a specific `(asset, timeframe)` pair.
    #[inline]
    #[must_use]
    pub fn get_coefficients(&self, asset: Asset, timeframe: Timeframe) -> &Coefficients {
        &self.coeffs[asset.index()][timeframe.index()]
    }
}

impl Default for LogisticModel {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl FairValueEstimator for LogisticModel {
    #[inline]
    fn estimate(
        &self,
        magnitude: f64,
        time_remaining_secs: u64,
        asset: Asset,
        timeframe: Timeframe,
    ) -> ContractPrice {
        let c = self.get_coefficients(asset, timeframe);
        // The casts are intentional: time values are well within f64 precision range
        // (max ~14400s), so precision loss is negligible in the probability context.
        #[expect(clippy::cast_precision_loss, reason = "time values fit in f64 for this domain")]
        let time_norm = time_remaining_secs as f64 / timeframe.duration_secs() as f64;
        let logit = c.beta_0
            + c.beta_1 * magnitude
            + c.beta_2 * time_norm
            + c.beta_3 * magnitude * time_norm;
        let prob = sigmoid(logit);
        // sigmoid always produces a value in (0, 1).
        // prob is in (0,1) by construction, so ContractPrice::new cannot return None here.
        #[expect(clippy::expect_used, reason = "sigmoid output is always in (0,1)")]
        ContractPrice::new(prob).expect("sigmoid output is always in (0,1)")
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use pm_types::{Asset, Timeframe};

    use super::*;

    #[test]
    fn default_model_returns_half() {
        let model = LogisticModel::new();
        let p = model
            .estimate(0.005, 120, Asset::Btc, Timeframe::Min5)
            .as_f64();
        assert!((p - 0.5).abs() < 1e-12, "expected 0.5, got {p}");
    }

    #[test]
    fn positive_beta1_increases_prob_with_magnitude() {
        let mut model = LogisticModel::new();
        model.set_coefficients(
            Asset::Btc,
            Timeframe::Hour1,
            Coefficients { beta_0: 0.0, beta_1: 50.0, beta_2: 0.0, beta_3: 0.0 },
        );
        let p_small = model.estimate(0.001, 300, Asset::Btc, Timeframe::Hour1).as_f64();
        let p_large = model.estimate(0.010, 300, Asset::Btc, Timeframe::Hour1).as_f64();
        assert!(p_large > p_small, "larger magnitude should yield higher prob");
        assert!(p_large > 0.5, "should be above neutral");
    }

    #[test]
    fn sigmoid_at_zero_is_half() {
        let s = sigmoid(0.0);
        assert!((s - 0.5).abs() < 1e-12);
    }

    #[test]
    fn sigmoid_large_positive_approaches_one() {
        let s = sigmoid(100.0);
        assert!(s > 0.999_999);
    }

    #[test]
    fn sigmoid_large_negative_approaches_zero() {
        let s = sigmoid(-100.0);
        assert!(s < 1e-6);
    }

    #[test]
    fn sigmoid_is_symmetric() {
        let x = 2.3;
        let s_pos = sigmoid(x);
        let s_neg = sigmoid(-x);
        assert!((s_pos + s_neg - 1.0).abs() < 1e-12);
    }

    #[test]
    fn set_and_get_coefficients_roundtrip() {
        let mut model = LogisticModel::new();
        let coeffs = Coefficients { beta_0: 0.1, beta_1: 2.5, beta_2: -0.3, beta_3: 0.0 };
        model.set_coefficients(Asset::Eth, Timeframe::Min15, coeffs);
        let got = model.get_coefficients(Asset::Eth, Timeframe::Min15);
        assert_eq!(*got, coeffs);
    }

    #[test]
    fn different_assets_use_independent_coefficients() {
        let mut model = LogisticModel::new();
        model.set_coefficients(
            Asset::Btc,
            Timeframe::Min5,
            Coefficients { beta_0: 1.0, beta_1: 0.0, beta_2: 0.0, beta_3: 0.0 },
        );
        // Eth still has default (all zeros)
        let btc_p = model.estimate(0.0, 60, Asset::Btc, Timeframe::Min5).as_f64();
        let eth_p = model.estimate(0.0, 60, Asset::Eth, Timeframe::Min5).as_f64();
        assert!(btc_p > 0.5 + 1e-6, "BTC with positive intercept should be > 0.5");
        assert!((eth_p - 0.5).abs() < 1e-12, "ETH default should be 0.5");
    }
}
