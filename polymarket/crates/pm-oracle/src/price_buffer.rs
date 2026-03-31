//! Per-asset circular price buffer with binary-search lookups.
//!
//! [`PriceBuffer`] maintains one [`AssetBuffer`] per [`Asset`] variant.  Each
//! buffer holds up to [`BUFFER_CAPACITY`] `(timestamp_ms, Price)` pairs in a
//! circular ring; older entries are silently overwritten once the ring is full.

use pm_types::{Asset, Price};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Maximum number of price entries held per asset before the oldest are
/// overwritten.
pub const BUFFER_CAPACITY: usize = 65_536;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Return a sentinel [`Price`] of `0.0` used to pre-fill the ring buffer.
///
/// `0.0` always satisfies the [`Price`] invariants (finite, non-negative).
/// This function is called exactly once per [`AssetBuffer`] construction.
fn sentinel_price() -> Price {
    // Price::new(0.0) can only return None for non-finite or negative values.
    // 0.0 is neither, so this branch is unreachable in practice.
    Price::new(0.0).unwrap_or_else(|| unreachable!("0.0 is a valid Price"))
}

// ─── AssetBuffer ─────────────────────────────────────────────────────────────

/// Circular ring buffer for `(timestamp_ms, Price)` pairs of a single asset.
pub struct AssetBuffer {
    data: Vec<(u64, Price)>,
    /// Write-head index (wraps around `BUFFER_CAPACITY`).
    head: usize,
    /// Number of valid entries (saturates at `BUFFER_CAPACITY`).
    len: usize,
}

impl AssetBuffer {
    fn new() -> Self {
        // `Price::new(0.0)` always succeeds: 0.0 is finite and non-negative.
        // We build the sentinel manually to avoid a fallible call in the hot init.
        let zero = sentinel_price();
        Self {
            data: vec![(0u64, zero); BUFFER_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    /// Append a new `(timestamp_ms, price)` entry.
    pub fn push(&mut self, timestamp_ms: u64, price: Price) {
        self.data[self.head] = (timestamp_ms, price);
        self.head = (self.head + 1) % BUFFER_CAPACITY;
        if self.len < BUFFER_CAPACITY {
            self.len += 1;
        }
    }

    /// Number of valid entries in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if no entries have been pushed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the most recently pushed price, or `None` if empty.
    #[must_use]
    pub fn latest(&self) -> Option<Price> {
        if self.len == 0 {
            return None;
        }
        // head points to the *next* write slot; the last written is one before.
        let last_idx = (self.head + BUFFER_CAPACITY - 1) % BUFFER_CAPACITY;
        Some(self.data[last_idx].1)
    }

    /// Return the price at or immediately before `timestamp_ms`.
    ///
    /// The search is a linear scan over the valid region because the buffer is
    /// circular and not guaranteed to be sorted after wrap-around.  For hot
    /// paths where this matters, callers should ensure data arrives in order,
    /// in which case the last few entries are almost always the answer.
    ///
    /// Returns `None` if the buffer is empty or if `timestamp_ms` is earlier
    /// than every stored timestamp.
    #[must_use]
    pub fn price_at(&self, timestamp_ms: u64) -> Option<Price> {
        if self.len == 0 {
            return None;
        }

        // Collect the valid entries in chronological order.
        // The oldest entry sits at `head` (wraps around after full).
        let oldest = if self.len < BUFFER_CAPACITY {
            0
        } else {
            self.head
        };

        // Build a logical slice of (ts, price) pairs ordered oldest → newest.
        // We use binary search on a reconstructed logical index to stay O(log N).
        // The ring stores [oldest..], wrapping, so logical[i] = data[(oldest+i) % CAP].
        let logical_get = |i: usize| -> (u64, Price) { self.data[(oldest + i) % BUFFER_CAPACITY] };

        // Binary search for the last entry with timestamp <= target.
        let mut lo: usize = 0;
        let mut hi: usize = self.len;
        // First check: if the very first entry is already after target, return None.
        if logical_get(0).0 > timestamp_ms {
            return None;
        }

        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            if logical_get(mid).0 <= timestamp_ms {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        Some(logical_get(lo).1)
    }
}

// ─── PriceBuffer ─────────────────────────────────────────────────────────────

/// Per-asset price buffer array covering all [`Asset`] variants.
///
/// Each asset gets its own [`AssetBuffer`] of capacity [`BUFFER_CAPACITY`].
pub struct PriceBuffer {
    buffers: [AssetBuffer; Asset::COUNT],
}

impl PriceBuffer {
    /// Construct a new, empty [`PriceBuffer`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffers: std::array::from_fn(|_| AssetBuffer::new()),
        }
    }

    /// Append a price tick for `asset`.
    pub fn push(&mut self, asset: Asset, timestamp_ms: u64, price: Price) {
        self.buffers[asset.index()].push(timestamp_ms, price);
    }

    /// Return the price at or immediately before `timestamp_ms` for `asset`.
    ///
    /// Returns `None` if the buffer for `asset` is empty or the timestamp
    /// precedes all stored entries.
    #[must_use]
    pub fn price_at(&self, asset: Asset, timestamp_ms: u64) -> Option<Price> {
        self.buffers[asset.index()].price_at(timestamp_ms)
    }

    /// Return the most recently pushed price for `asset`, or `None` if empty.
    #[must_use]
    pub fn latest(&self, asset: Asset) -> Option<Price> {
        self.buffers[asset.index()].latest()
    }

    /// Compute a weighted momentum score for `asset` relative to `current_price`.
    ///
    /// Samples the buffer at four lookback windows (30 s, 60 s, 120 s, 240 s) and
    /// computes a weighted average of the relative price change
    /// `(current - past) / past` at each horizon.  Only lookback windows for
    /// which a historical price is available contribute to the score.
    ///
    /// Returns `0.0` if no historical data is available for any lookback.
    #[must_use]
    pub fn momentum_score(&self, asset: Asset, now_ms: u64, current_price: f64) -> f64 {
        const LOOKBACKS_MS: [u64; 4] = [30_000, 60_000, 120_000, 240_000];
        const WEIGHTS: [f64; 4] = [0.15, 0.20, 0.30, 0.35];

        let mut score = 0.0;
        let mut total_weight = 0.0;

        for (&lb, &w) in LOOKBACKS_MS.iter().zip(WEIGHTS.iter()) {
            if let Some(past_price) = self.price_at(asset, now_ms.saturating_sub(lb)) {
                let past = past_price.as_f64();
                if past > 0.0 {
                    let slope = (current_price - past) / past;
                    score += slope * w;
                    total_weight += w;
                }
            }
        }

        if total_weight > 0.0 { score / total_weight } else { 0.0 }
    }
}

impl Default for PriceBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;

    fn p(v: f64) -> Price {
        Price::new(v).expect("valid price")
    }

    #[test]
    fn empty_buffer_latest_returns_none() {
        let buf = PriceBuffer::new();
        assert!(buf.latest(Asset::Btc).is_none());
    }

    #[test]
    fn empty_buffer_price_at_returns_none() {
        let buf = PriceBuffer::new();
        assert!(buf.price_at(Asset::Btc, 1_000_000).is_none());
    }

    #[test]
    fn single_entry_latest() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Btc, 1_000, p(42_000.0));
        let latest = buf.latest(Asset::Btc).expect("should have latest");
        assert!((latest.as_f64() - 42_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn single_entry_price_at_exact() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Eth, 1_000, p(3_000.0));
        let price = buf.price_at(Asset::Eth, 1_000).expect("exact match");
        assert!((price.as_f64() - 3_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn single_entry_price_at_after() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Eth, 1_000, p(3_000.0));
        // Timestamp after the only entry — should return that entry's price.
        let price = buf.price_at(Asset::Eth, 9_999).expect("price before");
        assert!((price.as_f64() - 3_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn single_entry_price_at_before_returns_none() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Sol, 1_000, p(150.0));
        assert!(buf.price_at(Asset::Sol, 500).is_none());
    }

    #[test]
    fn binary_search_mid_point() {
        let mut buf = PriceBuffer::new();
        // Push 5 entries at t = 0, 1000, 2000, 3000, 4000 ms.
        for i in 0u64..5 {
            buf.push(Asset::Btc, i * 1_000, p(100.0 + i as f64));
        }
        // Query at t = 2_500 → should return price at t = 2_000 (index 2).
        let price = buf.price_at(Asset::Btc, 2_500).expect("binary search");
        assert!((price.as_f64() - 102.0).abs() < f64::EPSILON);
    }

    #[test]
    fn binary_search_exact_match() {
        let mut buf = PriceBuffer::new();
        for i in 0u64..10 {
            buf.push(Asset::Btc, i * 100, p(i as f64 * 10.0));
        }
        // Exact match at t = 500 → price should be 50.0.
        let price = buf.price_at(Asset::Btc, 500).expect("exact match");
        assert!((price.as_f64() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn assets_are_independent() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Btc, 1_000, p(50_000.0));
        buf.push(Asset::Eth, 1_000, p(3_000.0));
        buf.push(Asset::Sol, 1_000, p(200.0));

        assert!((buf.latest(Asset::Btc).expect("btc").as_f64() - 50_000.0).abs() < f64::EPSILON);
        assert!((buf.latest(Asset::Eth).expect("eth").as_f64() - 3_000.0).abs() < f64::EPSILON);
        assert!((buf.latest(Asset::Sol).expect("sol").as_f64() - 200.0).abs() < f64::EPSILON);
        assert!(buf.latest(Asset::Xrp).is_none());
    }

    #[test]
    fn latest_returns_most_recent_after_multiple_pushes() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Xrp, 100, p(0.5));
        buf.push(Asset::Xrp, 200, p(0.6));
        buf.push(Asset::Xrp, 300, p(0.7));
        let latest = buf.latest(Asset::Xrp).expect("latest");
        assert!((latest.as_f64() - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn price_at_before_all_entries_returns_none() {
        let mut buf = PriceBuffer::new();
        buf.push(Asset::Btc, 10_000, p(1.0));
        buf.push(Asset::Btc, 20_000, p(2.0));
        // Timestamp before the first entry.
        assert!(buf.price_at(Asset::Btc, 5_000).is_none());
    }

    // ─── momentum_score tests ─────────────────────────────────────────────────

    #[test]
    fn momentum_score_empty_buffer_returns_zero() {
        let buf = PriceBuffer::new();
        let score = buf.momentum_score(Asset::Btc, 300_000, 50_000.0);
        assert_eq!(score, 0.0, "empty buffer should return 0.0");
    }

    #[test]
    fn momentum_score_uptrend_positive() {
        let mut buf = PriceBuffer::new();
        // Seed prices in the past so all four lookbacks resolve.
        // now_ms = 300_000; lookbacks at -30s, -60s, -120s, -240s.
        buf.push(Asset::Btc, 300_000 - 240_000, p(90_000.0));
        buf.push(Asset::Btc, 300_000 - 120_000, p(95_000.0));
        buf.push(Asset::Btc, 300_000 - 60_000,  p(97_000.0));
        buf.push(Asset::Btc, 300_000 - 30_000,  p(98_000.0));
        // Current price is 100_000 — all past prices are lower, so uptrend.
        let score = buf.momentum_score(Asset::Btc, 300_000, 100_000.0);
        assert!(score > 0.0, "uptrend should produce positive score, got {score}");
    }

    #[test]
    fn momentum_score_downtrend_negative() {
        let mut buf = PriceBuffer::new();
        // Past prices all higher than current — downtrend.
        buf.push(Asset::Eth, 300_000 - 240_000, p(4_000.0));
        buf.push(Asset::Eth, 300_000 - 120_000, p(3_800.0));
        buf.push(Asset::Eth, 300_000 - 60_000,  p(3_600.0));
        buf.push(Asset::Eth, 300_000 - 30_000,  p(3_200.0));
        let score = buf.momentum_score(Asset::Eth, 300_000, 3_000.0);
        assert!(score < 0.0, "downtrend should produce negative score, got {score}");
    }

    #[test]
    fn momentum_score_flat_returns_zero() {
        let mut buf = PriceBuffer::new();
        let price_val = 1_000.0;
        buf.push(Asset::Sol, 300_000 - 240_000, p(price_val));
        buf.push(Asset::Sol, 300_000 - 120_000, p(price_val));
        buf.push(Asset::Sol, 300_000 - 60_000,  p(price_val));
        buf.push(Asset::Sol, 300_000 - 30_000,  p(price_val));
        let score = buf.momentum_score(Asset::Sol, 300_000, price_val);
        assert!(score.abs() < f64::EPSILON, "flat price should produce score ≈ 0.0, got {score}");
    }

    #[test]
    fn momentum_score_partial_lookbacks_uses_available_data() {
        let mut buf = PriceBuffer::new();
        // Only push data at -30s; the other lookbacks will find the same entry
        // (price_at returns the last entry at or before the requested time).
        // We push a single entry far enough back that all lookbacks find it.
        buf.push(Asset::Xrp, 10_000, p(0.50));
        // now_ms = 300_000; current_price > past_price → positive score.
        let score = buf.momentum_score(Asset::Xrp, 300_000, 0.60);
        assert!(score > 0.0, "partial data with uptrend should be positive, got {score}");
    }
}
