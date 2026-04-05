//! Fixed-size order book indexed by tick offset.
//!
//! O(1) update, O(1) cached BBO. Suitable for bounded venues
//! like Polymarket (N=100) or Binance depth20 (N=20).

use mantis_events::UpdateAction;
use mantis_types::{Lots, Side, Ticks};

use super::OrderBook;

/// Fixed-size order book. Price levels indexed by tick offset.
///
/// Index 0 = lowest price, index N-1 = highest price.
/// For Polymarket: index 0 = 0.01, index 99 = 1.00 (tick = 0.01).
pub struct ArrayBook<const N: usize> {
    bids: [Lots; N],
    asks: [Lots; N],
    best_bid_idx: Option<u16>,
    best_ask_idx: Option<u16>,
    bid_dirty: bool,
    ask_dirty: bool,
}

impl<const N: usize> Default for ArrayBook<N> {
    fn default() -> Self {
        Self {
            bids: [Lots::ZERO; N],
            asks: [Lots::ZERO; N],
            best_bid_idx: None,
            best_ask_idx: None,
            bid_dirty: true,
            ask_dirty: true,
        }
    }
}

/// Convert a level index (usize) back to a Ticks price.
///
/// `N` is a compile-time const bounded to small values (≤ 65535) for supported venues.
/// `i64::try_from(idx)` is used to avoid sign-loss / wrap; we return `None` if conversion
/// somehow fails (should never happen with valid N).
fn idx_to_ticks(idx: usize) -> Option<Ticks> {
    i64::try_from(idx).ok().map(Ticks::from_raw)
}

impl<const N: usize> OrderBook for ArrayBook<N> {
    /// Action is intentionally ignored: for flat array books the qty IS the state.
    /// `Delete` callers pass `qty=Lots::ZERO`, which overwrites to zero — same effect.
    fn apply_delta(&mut self, price: Ticks, qty: Lots, side: Side, _action: UpdateAction) {
        let Ok(idx) = usize::try_from(price.to_raw()) else {
            return;
        };
        if idx >= N {
            return;
        }
        match side {
            Side::Bid => {
                self.bids[idx] = qty;
                self.bid_dirty = true;
            }
            Side::Ask => {
                self.asks[idx] = qty;
                self.ask_dirty = true;
            }
        }
    }

    fn clear_side(&mut self, side: Side) {
        match side {
            Side::Bid => {
                self.bids = [Lots::ZERO; N];
                self.best_bid_idx = None;
                self.bid_dirty = true;
            }
            Side::Ask => {
                self.asks = [Lots::ZERO; N];
                self.best_ask_idx = None;
                self.ask_dirty = true;
            }
        }
    }

    fn clear(&mut self) {
        self.bids = [Lots::ZERO; N];
        self.asks = [Lots::ZERO; N];
        self.best_bid_idx = None;
        self.best_ask_idx = None;
        self.bid_dirty = true;
        self.ask_dirty = true;
    }

    fn best_bid(&mut self) -> Option<(Ticks, Lots)> {
        if !self.bid_dirty {
            return self.best_bid_idx.map(|i| {
                let slot = usize::from(i);
                (Ticks::from_raw(i64::from(i)), self.bids[slot])
            });
        }
        // Scan from highest index downward for best bid
        self.best_bid_idx = None;
        for i in (0..N).rev() {
            if self.bids[i].to_raw() > 0 {
                // N ≤ 65535 for supported venues; truncation cannot happen
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "N bounded to u16 range for supported venues"
                )]
                {
                    self.best_bid_idx = Some(i as u16);
                }
                break;
            }
        }
        self.bid_dirty = false;
        self.best_bid_idx.map(|i| {
            let slot = usize::from(i);
            (Ticks::from_raw(i64::from(i)), self.bids[slot])
        })
    }

    fn best_ask(&mut self) -> Option<(Ticks, Lots)> {
        if !self.ask_dirty {
            return self.best_ask_idx.map(|i| {
                let slot = usize::from(i);
                (Ticks::from_raw(i64::from(i)), self.asks[slot])
            });
        }
        // Scan from lowest index upward for best ask
        self.best_ask_idx = None;
        for i in 0..N {
            if self.asks[i].to_raw() > 0 {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "N bounded to u16 range for supported venues"
                )]
                {
                    self.best_ask_idx = Some(i as u16);
                }
                break;
            }
        }
        self.ask_dirty = false;
        self.best_ask_idx.map(|i| {
            let slot = usize::from(i);
            (Ticks::from_raw(i64::from(i)), self.asks[slot])
        })
    }

    fn depth(&self, side: Side, buf: &mut [(Ticks, Lots)]) -> usize {
        let mut count = 0;
        match side {
            Side::Bid => {
                for i in (0..N).rev() {
                    if count >= buf.len() {
                        break;
                    }
                    if self.bids[i].to_raw() > 0 {
                        let Some(price) = idx_to_ticks(i) else {
                            continue;
                        };
                        buf[count] = (price, self.bids[i]);
                        count += 1;
                    }
                }
            }
            Side::Ask => {
                for i in 0..N {
                    if count >= buf.len() {
                        break;
                    }
                    if self.asks[i].to_raw() > 0 {
                        let Some(price) = idx_to_ticks(i) else {
                            continue;
                        };
                        buf[count] = (price, self.asks[i]);
                        count += 1;
                    }
                }
            }
        }
        count
    }

    fn level_count(&self, side: Side) -> usize {
        match side {
            Side::Bid => self.bids.iter().filter(|q| q.to_raw() > 0).count(),
            Side::Ask => self.asks.iter().filter(|q| q.to_raw() > 0).count(),
        }
    }

    fn total_depth(&self, side: Side, levels: usize) -> Lots {
        let mut total = Lots::ZERO;
        let mut count = 0;
        match side {
            Side::Bid => {
                for i in (0..N).rev() {
                    if count >= levels {
                        break;
                    }
                    if self.bids[i].to_raw() > 0 {
                        total += self.bids[i];
                        count += 1;
                    }
                }
            }
            Side::Ask => {
                for i in 0..N {
                    if count >= levels {
                        break;
                    }
                    if self.asks[i].to_raw() > 0 {
                        total += self.asks[i];
                        count += 1;
                    }
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_book_is_empty() {
        let mut book = ArrayBook::<100>::default();
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
        assert_eq!(book.level_count(Side::Bid), 0);
        assert_eq!(book.level_count(Side::Ask), 0);
    }

    #[test]
    fn apply_delta_new_level() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        assert_eq!(
            book.best_bid(),
            Some((Ticks::from_raw(45), Lots::from_raw(100)))
        );
    }

    #[test]
    fn apply_delta_delete_level() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::ZERO,
            Side::Bid,
            UpdateAction::Delete,
        );
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn best_bid_is_highest_price() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(40),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(200),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(42),
            Lots::from_raw(50),
            Side::Bid,
            UpdateAction::New,
        );
        assert_eq!(
            book.best_bid(),
            Some((Ticks::from_raw(45), Lots::from_raw(200)))
        );
    }

    #[test]
    fn best_ask_is_lowest_price() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(50),
            Lots::from_raw(100),
            Side::Ask,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(47),
            Lots::from_raw(200),
            Side::Ask,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(55),
            Lots::from_raw(50),
            Side::Ask,
            UpdateAction::New,
        );
        assert_eq!(
            book.best_ask(),
            Some((Ticks::from_raw(47), Lots::from_raw(200)))
        );
    }

    #[test]
    fn clear_resets_all() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(55),
            Lots::from_raw(100),
            Side::Ask,
            UpdateAction::New,
        );
        book.clear();
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn level_count_correct() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(40),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(200),
            Side::Bid,
            UpdateAction::New,
        );
        assert_eq!(book.level_count(Side::Bid), 2);
        assert_eq!(book.level_count(Side::Ask), 0);
    }

    #[test]
    fn total_depth_sums_top_n() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(44),
            Lots::from_raw(200),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(43),
            Lots::from_raw(300),
            Side::Bid,
            UpdateAction::New,
        );
        assert_eq!(book.total_depth(Side::Bid, 2), Lots::from_raw(300)); // top 2: 100 + 200
        assert_eq!(book.total_depth(Side::Bid, 5), Lots::from_raw(600)); // all 3: 100+200+300
    }

    #[test]
    fn depth_fills_buffer() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(44),
            Lots::from_raw(200),
            Side::Bid,
            UpdateAction::New,
        );
        let mut buf = [(Ticks::ZERO, Lots::ZERO); 5];
        let n = book.depth(Side::Bid, &mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf[0], (Ticks::from_raw(45), Lots::from_raw(100)));
        assert_eq!(buf[1], (Ticks::from_raw(44), Lots::from_raw(200)));
    }

    #[test]
    fn bbo_cache_returns_cached_on_second_call() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(55),
            Lots::from_raw(200),
            Side::Ask,
            UpdateAction::New,
        );

        // First call: scans and populates cache
        let bid1 = book.best_bid();
        let ask1 = book.best_ask();
        assert_eq!(bid1, Some((Ticks::from_raw(45), Lots::from_raw(100))));
        assert_eq!(ask1, Some((Ticks::from_raw(55), Lots::from_raw(200))));

        // Verify dirty flags are cleared (cache populated)
        assert!(!book.bid_dirty);
        assert!(!book.ask_dirty);

        // Second call: returns from cache (no scan)
        let bid2 = book.best_bid();
        let ask2 = book.best_ask();
        assert_eq!(bid1, bid2);
        assert_eq!(ask1, ask2);
    }

    #[test]
    fn bbo_cache_invalidated_on_delta() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(55),
            Lots::from_raw(200),
            Side::Ask,
            UpdateAction::New,
        );

        // Populate cache
        let _ = book.best_bid();
        let _ = book.best_ask();
        assert!(!book.bid_dirty);
        assert!(!book.ask_dirty);

        // New delta invalidates bid cache only
        book.apply_delta(
            Ticks::from_raw(46),
            Lots::from_raw(300),
            Side::Bid,
            UpdateAction::New,
        );
        assert!(book.bid_dirty);
        assert!(!book.ask_dirty);

        // Verify best_bid rescans and finds new best
        assert_eq!(
            book.best_bid(),
            Some((Ticks::from_raw(46), Lots::from_raw(300)))
        );
        assert!(!book.bid_dirty); // cache repopulated
    }

    #[test]
    fn bbo_cache_invalidated_on_clear() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(55),
            Lots::from_raw(200),
            Side::Ask,
            UpdateAction::New,
        );

        // Populate cache
        let _ = book.best_bid();
        assert!(!book.bid_dirty);

        // Clear invalidates cache
        book.clear();
        assert!(book.bid_dirty);
        assert!(book.ask_dirty);
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn bbo_cache_tracks_best_after_deletion() {
        let mut book = ArrayBook::<100>::default();
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::from_raw(100),
            Side::Bid,
            UpdateAction::New,
        );
        book.apply_delta(
            Ticks::from_raw(40),
            Lots::from_raw(200),
            Side::Bid,
            UpdateAction::New,
        );

        // Cache: best bid = 45
        assert_eq!(
            book.best_bid(),
            Some((Ticks::from_raw(45), Lots::from_raw(100)))
        );

        // Delete best bid — cache must invalidate and rescan to find 40
        book.apply_delta(
            Ticks::from_raw(45),
            Lots::ZERO,
            Side::Bid,
            UpdateAction::Delete,
        );
        assert!(book.bid_dirty);
        assert_eq!(
            book.best_bid(),
            Some((Ticks::from_raw(40), Lots::from_raw(200)))
        );
    }
}
