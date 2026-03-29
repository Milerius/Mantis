//! Empirical probability lookup table bucketed by (asset, timeframe, magnitude, `time_remaining`).

use pm_types::{Asset, ContractPrice, Timeframe};

use crate::estimator::FairValueEstimator;

// ─── Bucket boundaries ───────────────────────────────────────────────────────

/// Magnitude bucket upper boundaries (exclusive), as fractions.
///
/// E.g. `0.001` represents 0.1% move. There are `MAG_BUCKETS` total buckets
/// (including the overflow bucket beyond the last boundary).
const MAG_BOUNDARIES: [f64; 8] = [0.0, 0.001, 0.002, 0.003, 0.005, 0.008, 0.012, 0.020];

/// Time remaining bucket upper boundaries (exclusive), in seconds.
///
/// There are `TIME_BUCKETS` total buckets (including the overflow bucket).
const TIME_BOUNDARIES: [u64; 8] = [30, 60, 120, 180, 300, 480, 720, 900];

/// Number of magnitude buckets (boundaries + 1 overflow).
pub const MAG_BUCKETS: usize = MAG_BOUNDARIES.len() + 1;

/// Number of time buckets (boundaries + 1 overflow).
pub const TIME_BUCKETS: usize = TIME_BOUNDARIES.len() + 1;

/// Total number of cells in the flat array.
const TABLE_SIZE: usize = Asset::COUNT * Timeframe::COUNT * MAG_BUCKETS * TIME_BUCKETS;

// ─── LookupCell ──────────────────────────────────────────────────────────────

/// A single cell in the [`LookupTable`], storing an empirical probability and
/// the number of samples it was derived from.
#[derive(Debug, Clone, Copy)]
pub struct LookupCell {
    /// Empirical probability estimate (in `[0.0, 1.0]`).
    pub probability: f64,
    /// Number of historical samples backing this estimate.
    pub sample_count: u32,
}

impl Default for LookupCell {
    #[inline]
    fn default() -> Self {
        Self { probability: 0.5, sample_count: 0 }
    }
}

// ─── LookupTable ─────────────────────────────────────────────────────────────

/// Flat empirical probability table indexed by `(asset, timeframe, mag_bucket, time_bucket)`.
///
/// Cells with fewer than `min_samples` samples fall back to the prior of 0.5.
pub struct LookupTable {
    cells: [LookupCell; TABLE_SIZE],
    min_samples: u32,
}

impl LookupTable {
    /// Create an empty table. All cells default to probability 0.5, `sample_count` 0.
    ///
    /// The large stack allocation is intentional and sized by compile-time constants.
    #[inline]
    #[must_use]
    #[expect(clippy::large_stack_arrays, reason = "table is sized by const, intentional")]
    pub fn new(min_samples: u32) -> Self {
        Self { cells: [LookupCell::default(); TABLE_SIZE], min_samples }
    }

    /// Flat index for `(asset, timeframe, mag_bucket, time_bucket)`.
    #[inline]
    fn idx(asset: Asset, timeframe: Timeframe, mag_bucket: usize, time_bucket: usize) -> usize {
        asset.index() * (Timeframe::COUNT * MAG_BUCKETS * TIME_BUCKETS)
            + timeframe.index() * (MAG_BUCKETS * TIME_BUCKETS)
            + mag_bucket * TIME_BUCKETS
            + time_bucket
    }

    /// Set a cell's probability and sample count.
    ///
    /// The argument count exceeds the default clippy limit, but all parameters are
    /// necessary to address a single logical cell and no grouping is natural here.
    #[inline]
    #[expect(clippy::too_many_arguments, reason = "all args address a single cell; no grouping is natural")]
    pub fn set(
        &mut self,
        asset: Asset,
        timeframe: Timeframe,
        mag_bucket: usize,
        time_bucket: usize,
        probability: f64,
        sample_count: u32,
    ) {
        let i = Self::idx(asset, timeframe, mag_bucket, time_bucket);
        self.cells[i] = LookupCell { probability, sample_count };
    }

    /// Get a reference to the cell for the given indices.
    #[inline]
    #[must_use]
    pub fn get(
        &self,
        asset: Asset,
        timeframe: Timeframe,
        mag_bucket: usize,
        time_bucket: usize,
    ) -> &LookupCell {
        &self.cells[Self::idx(asset, timeframe, mag_bucket, time_bucket)]
    }

    /// Determine the magnitude bucket for a fractional magnitude value.
    ///
    /// Uses a linear scan over [`MAG_BOUNDARIES`] (8 elements — faster than binary search).
    #[inline]
    #[must_use]
    pub fn mag_bucket(magnitude: f64) -> usize {
        for (i, &boundary) in MAG_BOUNDARIES.iter().enumerate() {
            if magnitude <= boundary {
                return i;
            }
        }
        MAG_BOUNDARIES.len() // overflow bucket
    }

    /// Determine the time bucket for a seconds-remaining value.
    ///
    /// Uses a linear scan over [`TIME_BOUNDARIES`] (8 elements — faster than binary search).
    #[inline]
    #[must_use]
    pub fn time_bucket(time_remaining_secs: u64) -> usize {
        for (i, &boundary) in TIME_BOUNDARIES.iter().enumerate() {
            if time_remaining_secs <= boundary {
                return i;
            }
        }
        TIME_BOUNDARIES.len() // overflow bucket
    }
}

impl FairValueEstimator for LookupTable {
    #[inline]
    fn estimate(
        &self,
        magnitude: f64,
        time_remaining_secs: u64,
        asset: Asset,
        timeframe: Timeframe,
    ) -> ContractPrice {
        let mb = Self::mag_bucket(magnitude);
        let tb = Self::time_bucket(time_remaining_secs);
        let cell = self.get(asset, timeframe, mb, tb);
        let prob = if cell.sample_count >= self.min_samples { cell.probability } else { 0.5 };
        // prob is either 0.5 (valid by construction) or a stored probability.
        // Stored probabilities should be in [0,1]; fall back to 0.5 if they are not.
        ContractPrice::new(prob).unwrap_or_else(|| {
            // 0.5 is always a valid ContractPrice; this branch only fires if a stored
            // probability was somehow out of [0,1].
            #[expect(clippy::expect_used, reason = "0.5 is always a valid ContractPrice")]
            ContractPrice::new(0.5).expect("0.5 is a valid ContractPrice")
        })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use pm_types::{Asset, Timeframe};

    use super::*;

    // ── Bucket boundary tests ────────────────────────────────────────────────

    #[test]
    fn mag_bucket_zero_magnitude() {
        // 0.0 <= MAG_BOUNDARIES[0] (0.0), so bucket 0
        assert_eq!(LookupTable::mag_bucket(0.0), 0);
    }

    #[test]
    fn mag_bucket_first_boundary() {
        // exactly 0.001 → bucket 1 (first boundary that is >= 0.001 is index 1)
        assert_eq!(LookupTable::mag_bucket(0.001), 1);
    }

    #[test]
    fn mag_bucket_between_boundaries() {
        // 0.0015 is between 0.001 and 0.002; first boundary >= 0.0015 is 0.002 at index 2
        assert_eq!(LookupTable::mag_bucket(0.0015), 2);
    }

    #[test]
    fn mag_bucket_overflow() {
        // > all boundaries → overflow bucket
        assert_eq!(LookupTable::mag_bucket(0.5), MAG_BUCKETS - 1);
    }

    #[test]
    fn time_bucket_zero_secs() {
        // 0 <= TIME_BOUNDARIES[0] (30), so bucket 0
        assert_eq!(LookupTable::time_bucket(0), 0);
    }

    #[test]
    fn time_bucket_exactly_30() {
        assert_eq!(LookupTable::time_bucket(30), 0);
    }

    #[test]
    fn time_bucket_31_secs() {
        // 31 > 30, <= 60 → bucket 1
        assert_eq!(LookupTable::time_bucket(31), 1);
    }

    #[test]
    fn time_bucket_overflow() {
        // > all boundaries
        assert_eq!(LookupTable::time_bucket(9999), TIME_BUCKETS - 1);
    }

    // ── Table behaviour tests ────────────────────────────────────────────────

    #[test]
    fn empty_table_returns_half() {
        let table = LookupTable::new(10);
        let p = table
            .estimate(0.001, 60, Asset::Btc, Timeframe::Min5)
            .as_f64();
        assert!((p - 0.5).abs() < 1e-12, "expected 0.5, got {p}");
    }

    #[test]
    fn populated_cell_returns_value_when_samples_sufficient() {
        let mut table = LookupTable::new(5);
        let mb = LookupTable::mag_bucket(0.0015);
        let tb = LookupTable::time_bucket(90);
        table.set(Asset::Btc, Timeframe::Min5, mb, tb, 0.72, 20);

        let p = table
            .estimate(0.0015, 90, Asset::Btc, Timeframe::Min5)
            .as_f64();
        assert!((p - 0.72).abs() < 1e-12, "expected 0.72, got {p}");
    }

    #[test]
    fn low_sample_count_falls_back_to_half() {
        let mut table = LookupTable::new(50);
        let mb = LookupTable::mag_bucket(0.0015);
        let tb = LookupTable::time_bucket(90);
        table.set(Asset::Btc, Timeframe::Min5, mb, tb, 0.72, 10); // 10 < 50

        let p = table
            .estimate(0.0015, 90, Asset::Btc, Timeframe::Min5)
            .as_f64();
        assert!((p - 0.5).abs() < 1e-12, "expected 0.5 (fallback), got {p}");
    }

    #[test]
    fn different_assets_are_independent() {
        let mut table = LookupTable::new(1);
        let mb = LookupTable::mag_bucket(0.002);
        let tb = LookupTable::time_bucket(120);

        table.set(Asset::Btc, Timeframe::Hour1, mb, tb, 0.65, 10);
        table.set(Asset::Eth, Timeframe::Hour1, mb, tb, 0.40, 10);

        let btc_p = table
            .estimate(0.002, 120, Asset::Btc, Timeframe::Hour1)
            .as_f64();
        let eth_p = table
            .estimate(0.002, 120, Asset::Eth, Timeframe::Hour1)
            .as_f64();

        assert!((btc_p - 0.65).abs() < 1e-12, "BTC expected 0.65, got {btc_p}");
        assert!((eth_p - 0.40).abs() < 1e-12, "ETH expected 0.40, got {eth_p}");
    }

    #[test]
    fn different_timeframes_are_independent() {
        let mut table = LookupTable::new(1);
        let mb = LookupTable::mag_bucket(0.003);
        let tb = LookupTable::time_bucket(60);

        table.set(Asset::Sol, Timeframe::Min5, mb, tb, 0.55, 5);
        table.set(Asset::Sol, Timeframe::Hour4, mb, tb, 0.45, 5);

        let p5 = table
            .estimate(0.003, 60, Asset::Sol, Timeframe::Min5)
            .as_f64();
        let p4h = table
            .estimate(0.003, 60, Asset::Sol, Timeframe::Hour4)
            .as_f64();

        assert!((p5 - 0.55).abs() < 1e-12);
        assert!((p4h - 0.45).abs() < 1e-12);
    }
}
