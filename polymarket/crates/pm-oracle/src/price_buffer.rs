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
}

impl Default for PriceBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
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
}
