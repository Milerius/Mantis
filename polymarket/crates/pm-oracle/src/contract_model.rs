//! Empirical model mapping (`spot_magnitude`, `time_elapsed`) → estimated contract price.
//!
//! Calibrated from paired Binance spot + Polymarket trade data.
//! For windows where we have actual Polymarket trade data, we use that directly.
//! For windows without data, this model provides an estimate.

use pm_types::{Asset, ContractPrice, Timeframe};

// ─── Bucket boundaries ───────────────────────────────────────────────────────

/// Time elapsed bucket boundaries (seconds since window open).
pub const TIME_ELAPSED_BUCKETS: &[u64] = &[0, 30, 60, 120, 180, 300, 480, 720];
/// Number of time buckets (boundaries + 1 overflow).
pub const TIME_ELAPSED_BUCKET_COUNT: usize = TIME_ELAPSED_BUCKETS.len() + 1;

/// Spot magnitude bucket boundaries (as fractions, not percentages).
pub const MAGNITUDE_BUCKETS: &[f64] = &[0.0, 0.0005, 0.001, 0.002, 0.003, 0.005, 0.008, 0.012];
/// Number of magnitude buckets (boundaries + 1 overflow).
pub const MAGNITUDE_BUCKET_COUNT: usize = MAGNITUDE_BUCKETS.len() + 1;

/// Total cells per (asset, timeframe) pair.
const CELLS_PER_PAIR: usize = MAGNITUDE_BUCKET_COUNT * TIME_ELAPSED_BUCKET_COUNT;
/// Total cells in the model.
const TOTAL_CELLS: usize = Asset::COUNT * Timeframe::COUNT * CELLS_PER_PAIR;

// ─── ModelCell ───────────────────────────────────────────────────────────────

/// A single cell storing median contract price and sample count.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModelCell {
    /// Median contract price observed for this (magnitude, `time_elapsed`) bucket.
    pub median_price: f64,
    /// Number of observations in this bucket.
    pub sample_count: u32,
}

// ─── ContractPriceModel ──────────────────────────────────────────────────────

/// Empirical contract price model.
///
/// Answers: "Given that spot moved X% in the first Y seconds of a window,
/// what is the typical Polymarket contract price?"
///
/// Calibrated from real Polymarket trade data paired with Binance spot data.
pub struct ContractPriceModel {
    cells: Vec<ModelCell>,
    min_samples: u32,
}

impl ContractPriceModel {
    /// Create a new empty model with default values (0.5 = no info).
    #[must_use]
    pub fn new(min_samples: u32) -> Self {
        Self {
            cells: vec![ModelCell::default(); TOTAL_CELLS],
            min_samples,
        }
    }

    /// Flat index for `(asset, timeframe, mag_bucket, time_bucket)`.
    #[inline]
    pub(crate) fn idx(asset: Asset, tf: Timeframe, mag_bucket: usize, time_bucket: usize) -> usize {
        asset.index() * Timeframe::COUNT * CELLS_PER_PAIR
            + tf.index() * CELLS_PER_PAIR
            + mag_bucket * TIME_ELAPSED_BUCKET_COUNT
            + time_bucket
    }

    /// Set a cell's value.
    #[expect(
        clippy::too_many_arguments,
        reason = "all args address a single cell; no grouping is natural"
    )]
    pub fn set(
        &mut self,
        asset: Asset,
        tf: Timeframe,
        mag_bucket: usize,
        time_bucket: usize,
        median_price: f64,
        sample_count: u32,
    ) {
        let i = Self::idx(asset, tf, mag_bucket, time_bucket);
        self.cells[i] = ModelCell {
            median_price,
            sample_count,
        };
    }

    /// Get a cell.
    #[must_use]
    pub fn get(&self, asset: Asset, tf: Timeframe, mag_bucket: usize, time_bucket: usize) -> &ModelCell {
        &self.cells[Self::idx(asset, tf, mag_bucket, time_bucket)]
    }

    /// Find magnitude bucket for a value (linear scan, small array).
    ///
    /// Returns the index of the first boundary that the value does not exceed,
    /// or the overflow bucket if the value exceeds all boundaries.
    #[inline]
    #[must_use]
    pub fn mag_bucket(magnitude: f64) -> usize {
        for (i, &boundary) in MAGNITUDE_BUCKETS.iter().enumerate() {
            if magnitude <= boundary {
                return i;
            }
        }
        MAGNITUDE_BUCKETS.len() // overflow bucket
    }

    /// Find time elapsed bucket for a value (linear scan, small array).
    ///
    /// Returns the index of the first boundary that the value does not exceed,
    /// or the overflow bucket if the value exceeds all boundaries.
    #[inline]
    #[must_use]
    pub fn time_bucket(time_elapsed_secs: u64) -> usize {
        for (i, &boundary) in TIME_ELAPSED_BUCKETS.iter().enumerate() {
            if time_elapsed_secs <= boundary {
                return i;
            }
        }
        TIME_ELAPSED_BUCKETS.len() // overflow bucket
    }

    /// Estimate the contract price for the given (magnitude, `time_elapsed`, asset, timeframe).
    ///
    /// Returns `None` if the cell has insufficient data (fewer than `min_samples`).
    /// Returns `Some(0.5)` when the model is empty and `min_samples` is 0, since
    /// the default cell median is 0.0 which would produce `Some(0.0)` — callers
    /// that want the uninformative prior should use `min_samples > 0`.
    #[must_use]
    pub fn estimate(
        &self,
        magnitude: f64,
        time_elapsed_secs: u64,
        asset: Asset,
        timeframe: Timeframe,
    ) -> Option<ContractPrice> {
        let mb = Self::mag_bucket(magnitude);
        let tb = Self::time_bucket(time_elapsed_secs);
        let cell = self.get(asset, timeframe, mb, tb);
        if cell.sample_count < self.min_samples {
            return None;
        }
        ContractPrice::new(cell.median_price)
    }
}

// ─── Calibration ─────────────────────────────────────────────────────────────

/// An observation pairing spot movement with a Polymarket trade price.
pub struct PriceObservation {
    /// The asset this observation belongs to.
    pub asset: Asset,
    /// The prediction window timeframe.
    pub timeframe: Timeframe,
    /// Spot magnitude at this moment (as a fraction, not a percentage).
    pub magnitude: f64,
    /// Seconds elapsed since the window opened.
    pub time_elapsed_secs: u64,
    /// Actual Polymarket trade price (in `[0.0, 1.0]`).
    pub contract_price: f64,
}

/// Build a model from a set of observations.
///
/// For each (asset, timeframe, `mag_bucket`, `time_bucket`) cell the function
/// collects all matching contract prices, sorts them, and stores the median.
/// Cells with no observations retain the default [`ModelCell`] (median 0.0,
/// `sample_count` 0).
#[must_use]
pub fn calibrate(observations: &[PriceObservation], min_samples: u32) -> ContractPriceModel {
    // Accumulate prices per cell using a flat Vec<Vec<f64>>.
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); TOTAL_CELLS];

    for obs in observations {
        let mb = ContractPriceModel::mag_bucket(obs.magnitude);
        let tb = ContractPriceModel::time_bucket(obs.time_elapsed_secs);
        let i = ContractPriceModel::idx(obs.asset, obs.timeframe, mb, tb);
        buckets[i].push(obs.contract_price);
    }

    let mut model = ContractPriceModel::new(min_samples);

    for (i, prices) in buckets.iter_mut().enumerate() {
        if prices.is_empty() {
            continue;
        }
        let count = prices.len();
        prices.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        let median = if count % 2 == 1 {
            prices[count / 2]
        } else {
            // Safe: f64 midpoint via average; inputs are finite contract prices in [0,1].
            let lo = prices[count / 2 - 1];
            let hi = prices[count / 2];
            lo + (hi - lo) / 2.0
        };
        model.cells[i] = ModelCell {
            median_price: median,
            sample_count: u32::try_from(count).unwrap_or(u32::MAX),
        };
    }

    model
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions intentionally panic on failure")]
mod tests {
    use pm_types::{Asset, Timeframe};

    use super::*;

    // ── Empty model returns None (min_samples > 0) ───────────────────────────

    #[test]
    fn empty_model_with_min_samples_returns_none() {
        let model = ContractPriceModel::new(1);
        let result = model.estimate(0.001, 60, Asset::Btc, Timeframe::Min5);
        assert!(result.is_none(), "expected None from empty model with min_samples=1");
    }

    #[test]
    fn empty_model_all_assets_return_none() {
        let model = ContractPriceModel::new(1);
        for asset in Asset::ALL {
            for tf in Timeframe::ALL {
                let result = model.estimate(0.002, 90, asset, tf);
                assert!(result.is_none(), "expected None for {asset}/{tf}");
            }
        }
    }

    // ── Populated cell returns stored value ──────────────────────────────────

    #[test]
    fn populated_cell_returns_stored_value() {
        let mut model = ContractPriceModel::new(1);
        let mb = ContractPriceModel::mag_bucket(0.001);
        let tb = ContractPriceModel::time_bucket(60);
        model.set(Asset::Btc, Timeframe::Min15, mb, tb, 0.72, 10);

        let result = model.estimate(0.001, 60, Asset::Btc, Timeframe::Min15);
        let price = result.expect("should return value").as_f64();
        assert!((price - 0.72).abs() < 1e-12, "expected 0.72, got {price}");
    }

    #[test]
    fn populated_cell_exact_boundary_values() {
        let mut model = ContractPriceModel::new(5);
        let mb = ContractPriceModel::mag_bucket(0.005);
        let tb = ContractPriceModel::time_bucket(300);
        model.set(Asset::Eth, Timeframe::Hour1, mb, tb, 0.63, 20);

        let result = model.estimate(0.005, 300, Asset::Eth, Timeframe::Hour1);
        let price = result.expect("should return value").as_f64();
        assert!((price - 0.63).abs() < 1e-12, "expected 0.63, got {price}");
    }

    // ── Low-sample cell returns None ─────────────────────────────────────────

    #[test]
    fn low_sample_cell_returns_none() {
        let mut model = ContractPriceModel::new(50);
        let mb = ContractPriceModel::mag_bucket(0.002);
        let tb = ContractPriceModel::time_bucket(120);
        model.set(Asset::Sol, Timeframe::Min5, mb, tb, 0.68, 10); // 10 < 50

        let result = model.estimate(0.002, 120, Asset::Sol, Timeframe::Min5);
        assert!(result.is_none(), "expected None for low-sample cell");
    }

    #[test]
    fn exactly_min_samples_returns_value() {
        let mut model = ContractPriceModel::new(10);
        let mb = ContractPriceModel::mag_bucket(0.003);
        let tb = ContractPriceModel::time_bucket(180);
        model.set(Asset::Xrp, Timeframe::Hour4, mb, tb, 0.55, 10); // exactly 10

        let result = model.estimate(0.003, 180, Asset::Xrp, Timeframe::Hour4);
        let price = result.expect("should return value at exactly min_samples").as_f64();
        assert!((price - 0.55).abs() < 1e-12);
    }

    // ── Magnitude bucket boundary tests ──────────────────────────────────────

    #[test]
    fn mag_bucket_zero_magnitude() {
        // 0.0 <= MAGNITUDE_BUCKETS[0] (0.0), so bucket 0
        assert_eq!(ContractPriceModel::mag_bucket(0.0), 0);
    }

    #[test]
    fn mag_bucket_first_positive_boundary() {
        // 0.0005 <= MAGNITUDE_BUCKETS[1] (0.0005), so bucket 1
        assert_eq!(ContractPriceModel::mag_bucket(0.0005), 1);
    }

    #[test]
    fn mag_bucket_between_boundaries() {
        // 0.00075 is between 0.0005 and 0.001; first boundary >= 0.00075 is 0.001 at index 2
        assert_eq!(ContractPriceModel::mag_bucket(0.00075), 2);
    }

    #[test]
    fn mag_bucket_overflow() {
        // > all boundaries → overflow bucket
        assert_eq!(ContractPriceModel::mag_bucket(1.0), MAGNITUDE_BUCKET_COUNT - 1);
    }

    #[test]
    fn mag_bucket_last_boundary() {
        // 0.012 <= MAGNITUDE_BUCKETS[7] (0.012)
        assert_eq!(ContractPriceModel::mag_bucket(0.012), 7);
    }

    // ── Time elapsed bucket boundary tests ───────────────────────────────────

    #[test]
    fn time_bucket_zero_secs() {
        // 0 <= TIME_ELAPSED_BUCKETS[0] (0), so bucket 0
        assert_eq!(ContractPriceModel::time_bucket(0), 0);
    }

    #[test]
    fn time_bucket_exactly_30() {
        // 30 <= TIME_ELAPSED_BUCKETS[1] (30), so bucket 1
        assert_eq!(ContractPriceModel::time_bucket(30), 1);
    }

    #[test]
    fn time_bucket_between_30_and_60() {
        // 45 > 30, <= 60 → bucket 2
        assert_eq!(ContractPriceModel::time_bucket(45), 2);
    }

    #[test]
    fn time_bucket_overflow() {
        // > all boundaries → overflow bucket
        assert_eq!(ContractPriceModel::time_bucket(9999), TIME_ELAPSED_BUCKET_COUNT - 1);
    }

    #[test]
    fn time_bucket_last_boundary() {
        // 720 <= TIME_ELAPSED_BUCKETS[7] (720), so bucket 7
        assert_eq!(ContractPriceModel::time_bucket(720), 7);
    }

    // ── Calibration produces correct medians ─────────────────────────────────

    #[test]
    fn calibrate_single_observation() {
        let obs = vec![PriceObservation {
            asset: Asset::Btc,
            timeframe: Timeframe::Min15,
            magnitude: 0.001,
            time_elapsed_secs: 60,
            contract_price: 0.70,
        }];
        let model = calibrate(&obs, 1);
        let mb = ContractPriceModel::mag_bucket(0.001);
        let tb = ContractPriceModel::time_bucket(60);
        let cell = model.get(Asset::Btc, Timeframe::Min15, mb, tb);
        assert_eq!(cell.sample_count, 1);
        assert!((cell.median_price - 0.70).abs() < 1e-12);
    }

    #[test]
    fn calibrate_odd_count_median() {
        // Three prices: 0.60, 0.70, 0.80 → median = 0.70
        let obs: Vec<PriceObservation> = [0.60_f64, 0.70, 0.80]
            .iter()
            .map(|&p| PriceObservation {
                asset: Asset::Btc,
                timeframe: Timeframe::Min5,
                magnitude: 0.002,
                time_elapsed_secs: 120,
                contract_price: p,
            })
            .collect();
        let model = calibrate(&obs, 1);
        let mb = ContractPriceModel::mag_bucket(0.002);
        let tb = ContractPriceModel::time_bucket(120);
        let cell = model.get(Asset::Btc, Timeframe::Min5, mb, tb);
        assert_eq!(cell.sample_count, 3);
        assert!((cell.median_price - 0.70).abs() < 1e-12, "expected 0.70, got {}", cell.median_price);
    }

    #[test]
    fn calibrate_even_count_median() {
        // Four prices: 0.60, 0.65, 0.75, 0.80 → median = (0.65 + 0.75) / 2 = 0.70
        let obs: Vec<PriceObservation> = [0.60_f64, 0.65, 0.75, 0.80]
            .iter()
            .map(|&p| PriceObservation {
                asset: Asset::Eth,
                timeframe: Timeframe::Hour1,
                magnitude: 0.003,
                time_elapsed_secs: 180,
                contract_price: p,
            })
            .collect();
        let model = calibrate(&obs, 1);
        let mb = ContractPriceModel::mag_bucket(0.003);
        let tb = ContractPriceModel::time_bucket(180);
        let cell = model.get(Asset::Eth, Timeframe::Hour1, mb, tb);
        assert_eq!(cell.sample_count, 4);
        assert!((cell.median_price - 0.70).abs() < 1e-12, "expected 0.70, got {}", cell.median_price);
    }

    #[test]
    fn calibrate_empty_observations_returns_empty_model() {
        let model = calibrate(&[], 1);
        let result = model.estimate(0.001, 60, Asset::Btc, Timeframe::Min5);
        assert!(result.is_none(), "empty calibration should yield no estimates");
    }

    #[test]
    fn calibrate_out_of_order_prices_correct_median() {
        // Prices given in reverse order; calibrate must sort before taking median.
        let obs: Vec<PriceObservation> = [0.90_f64, 0.40, 0.70]
            .iter()
            .map(|&p| PriceObservation {
                asset: Asset::Sol,
                timeframe: Timeframe::Hour4,
                magnitude: 0.008,
                time_elapsed_secs: 480,
                contract_price: p,
            })
            .collect();
        let model = calibrate(&obs, 1);
        let result = model.estimate(0.008, 480, Asset::Sol, Timeframe::Hour4);
        let price = result.expect("should return value").as_f64();
        assert!((price - 0.70).abs() < 1e-12, "expected 0.70, got {price}");
    }

    // ── Assets are independent ───────────────────────────────────────────────

    #[test]
    fn different_assets_are_independent() {
        let mut model = ContractPriceModel::new(1);
        let mb = ContractPriceModel::mag_bucket(0.002);
        let tb = ContractPriceModel::time_bucket(120);

        model.set(Asset::Btc, Timeframe::Hour1, mb, tb, 0.65, 10);
        model.set(Asset::Eth, Timeframe::Hour1, mb, tb, 0.40, 10);

        let btc = model.estimate(0.002, 120, Asset::Btc, Timeframe::Hour1).expect("btc").as_f64();
        let eth = model.estimate(0.002, 120, Asset::Eth, Timeframe::Hour1).expect("eth").as_f64();

        assert!((btc - 0.65).abs() < 1e-12, "BTC expected 0.65, got {btc}");
        assert!((eth - 0.40).abs() < 1e-12, "ETH expected 0.40, got {eth}");
    }

    #[test]
    fn calibrate_assets_are_independent() {
        let obs = vec![
            PriceObservation {
                asset: Asset::Btc,
                timeframe: Timeframe::Min15,
                magnitude: 0.001,
                time_elapsed_secs: 60,
                contract_price: 0.75,
            },
            PriceObservation {
                asset: Asset::Sol,
                timeframe: Timeframe::Min15,
                magnitude: 0.001,
                time_elapsed_secs: 60,
                contract_price: 0.45,
            },
        ];
        let model = calibrate(&obs, 1);
        let btc = model.estimate(0.001, 60, Asset::Btc, Timeframe::Min15).expect("btc").as_f64();
        let sol = model.estimate(0.001, 60, Asset::Sol, Timeframe::Min15).expect("sol").as_f64();
        let eth = model.estimate(0.001, 60, Asset::Eth, Timeframe::Min15);

        assert!((btc - 0.75).abs() < 1e-12, "BTC expected 0.75, got {btc}");
        assert!((sol - 0.45).abs() < 1e-12, "SOL expected 0.45, got {sol}");
        assert!(eth.is_none(), "ETH should have no data");
    }

    // ── Timeframes are independent ───────────────────────────────────────────

    #[test]
    fn different_timeframes_are_independent() {
        let mut model = ContractPriceModel::new(1);
        let mb = ContractPriceModel::mag_bucket(0.003);
        let tb = ContractPriceModel::time_bucket(60);

        model.set(Asset::Sol, Timeframe::Min5, mb, tb, 0.55, 5);
        model.set(Asset::Sol, Timeframe::Hour4, mb, tb, 0.45, 5);

        let p5 = model.estimate(0.003, 60, Asset::Sol, Timeframe::Min5).expect("Min5").as_f64();
        let p4h = model.estimate(0.003, 60, Asset::Sol, Timeframe::Hour4).expect("Hour4").as_f64();

        assert!((p5 - 0.55).abs() < 1e-12, "Min5 expected 0.55, got {p5}");
        assert!((p4h - 0.45).abs() < 1e-12, "Hour4 expected 0.45, got {p4h}");
    }

    // ── Overflow buckets accept extreme values ───────────────────────────────

    #[test]
    fn overflow_mag_bucket_routes_correctly() {
        let mut model = ContractPriceModel::new(1);
        let mb = MAGNITUDE_BUCKET_COUNT - 1; // overflow
        let tb = ContractPriceModel::time_bucket(30);
        model.set(Asset::Xrp, Timeframe::Min15, mb, tb, 0.80, 5);

        // Any magnitude > 0.012 should land in the overflow bucket.
        let result = model.estimate(0.999, 30, Asset::Xrp, Timeframe::Min15);
        let price = result.expect("overflow mag should route").as_f64();
        assert!((price - 0.80).abs() < 1e-12);
    }

    #[test]
    fn overflow_time_bucket_routes_correctly() {
        let mut model = ContractPriceModel::new(1);
        let mb = ContractPriceModel::mag_bucket(0.001);
        let tb = TIME_ELAPSED_BUCKET_COUNT - 1; // overflow
        model.set(Asset::Btc, Timeframe::Hour4, mb, tb, 0.60, 5);

        // Any time > 720 should land in the overflow bucket.
        let result = model.estimate(0.001, 99_999, Asset::Btc, Timeframe::Hour4);
        let price = result.expect("overflow time should route").as_f64();
        assert!((price - 0.60).abs() < 1e-12);
    }
}
