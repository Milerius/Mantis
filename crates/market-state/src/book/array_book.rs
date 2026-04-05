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

    fn best_bid(&self) -> Option<(Ticks, Lots)> {
        if !self.bid_dirty {
            return self.best_bid_idx.map(|i| {
                let slot = usize::from(i);
                (Ticks::from_raw(i64::from(i)), self.bids[slot])
            });
        }
        // Scan from highest index downward for best bid
        for i in (0..N).rev() {
            if self.bids[i].to_raw() > 0 {
                let Some(price) = idx_to_ticks(i) else {
                    continue;
                };
                return Some((price, self.bids[i]));
            }
        }
        None
    }

    fn best_ask(&self) -> Option<(Ticks, Lots)> {
        if !self.ask_dirty {
            return self.best_ask_idx.map(|i| {
                let slot = usize::from(i);
                (Ticks::from_raw(i64::from(i)), self.asks[slot])
            });
        }
        // Scan from lowest index upward for best ask
        for i in 0..N {
            if self.asks[i].to_raw() > 0 {
                let Some(price) = idx_to_ticks(i) else {
                    continue;
                };
                return Some((price, self.asks[i]));
            }
        }
        None
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
