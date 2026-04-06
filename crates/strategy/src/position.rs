//! Generic signed position with `VWAP` entry tracking and `PnL` accounting.

use mantis_fixed::FixedI64;
use mantis_types::{InstrumentId, Lots, SignedLots, Side};

/// Signed inventory position with VWAP average entry price.
///
/// Tracks a single instrument's net position (long positive, short negative),
/// computes a volume-weighted average entry price on increases, and realizes
/// `PnL` when the position is reduced or flipped.
///
/// ## Invariants
///
/// - `qty.is_zero()` implies `avg_entry == FixedI64::ZERO`.
/// - `avg_entry` is always non-negative.
/// - `fill_count` monotonically increases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Position {
    /// Instrument this position belongs to.
    pub instrument_id: InstrumentId,
    /// Net signed lot quantity: positive = long, negative = short.
    pub qty: SignedLots,
    /// Volume-weighted average entry price. Zero when flat.
    pub avg_entry: FixedI64<6>,
    /// Cumulative realized `PnL` since position was opened.
    pub realized_pnl: FixedI64<6>,
    /// Total number of fills processed.
    pub fill_count: u32,
}

impl Position {
    /// Create a new flat position for `instrument_id`.
    #[must_use]
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_id,
            qty: SignedLots::ZERO,
            avg_entry: FixedI64::ZERO,
            realized_pnl: FixedI64::ZERO,
            fill_count: 0,
        }
    }

    /// Process a fill: update `VWAP` on position increase, realize `PnL` on decrease.
    ///
    /// - `side`: direction of the fill (Bid = buy, Ask = sell).
    /// - `qty`: unsigned lot size of the fill.
    /// - `price`: fill price.
    ///
    /// When position increases (same direction as current or from flat):
    ///   `avg_entry = (old_notional + new_notional) / new_total_qty`
    ///
    /// When position decreases (opposite direction):
    ///   `realized_pnl += closed_qty * (price - avg_entry) * direction`
    ///
    /// When position reaches zero, `avg_entry` is set to `ZERO`.
    pub fn on_fill(&mut self, side: Side, qty: Lots, price: FixedI64<6>) {
        // Convert fill to signed delta: Bid = +qty, Ask = -qty.
        let delta = match side {
            Side::Bid => SignedLots::from(qty),
            Side::Ask => -SignedLots::from(qty),
        };

        let old_qty = self.qty;
        let new_qty = old_qty + delta;

        // Determine whether this fill increases or decreases the position.
        // Increasing: same sign as delta (or was flat), i.e. we are adding to the position.
        let increasing = old_qty.is_zero()
            || (old_qty.to_raw() > 0 && delta.to_raw() > 0)
            || (old_qty.to_raw() < 0 && delta.to_raw() < 0);

        if increasing {
            // Update VWAP: avg_entry = (old_qty * avg_entry + |delta| * price) / |new_qty|
            let old_qty_raw = old_qty.to_raw().abs();
            let delta_raw = delta.to_raw().abs();
            let new_qty_raw = new_qty.to_raw().abs();

            // old_cost = old_qty * avg_entry (both positive scalars)
            let old_cost = self
                .avg_entry
                .checked_mul_int(old_qty_raw)
                .unwrap_or(FixedI64::ZERO);
            // new_cost = |delta| * price
            let new_cost = price
                .checked_mul_int(delta_raw)
                .unwrap_or(FixedI64::ZERO);
            // total_cost = old_cost + new_cost
            let total_cost = old_cost
                .checked_add(new_cost)
                .unwrap_or(FixedI64::ZERO);
            // avg_entry = total_cost / new_total_qty
            self.avg_entry = total_cost
                .checked_div_int(new_qty_raw)
                .unwrap_or(FixedI64::ZERO);
        } else {
            // Reducing position — realize PnL.
            // closed_qty is the min of |delta| and |old_qty| (we only close, not flip here).
            let closed_qty = delta.to_raw().abs().min(old_qty.to_raw().abs());
            // direction: +1 if we were long, -1 if we were short.
            let direction = old_qty.signum(); // 1 or -1

            // pnl_per_lot = (price - avg_entry) * direction
            // For a long: pnl = closed_qty * (price - avg_entry)
            // For a short: pnl = closed_qty * (avg_entry - price)  => direction = -1
            let pnl_per_lot = price
                .checked_sub(self.avg_entry)
                .unwrap_or(FixedI64::ZERO)
                .checked_mul_int(direction)
                .unwrap_or(FixedI64::ZERO);
            let fill_pnl = pnl_per_lot
                .checked_mul_int(closed_qty)
                .unwrap_or(FixedI64::ZERO);
            self.realized_pnl = self
                .realized_pnl
                .checked_add(fill_pnl)
                .unwrap_or(self.realized_pnl);

            // If the fill crosses flat (flip), the remaining delta opens a new position.
            // avg_entry for the new leg is the fill price.
            if new_qty.to_raw().abs() > 0
                && new_qty.signum() != old_qty.signum()
            {
                // Flipped — new avg_entry is the fill price.
                self.avg_entry = price;
            }
            // If flat or same sign reduced, avg_entry stays unless we are now flat.
        }

        self.qty = new_qty;
        self.fill_count = self.fill_count.saturating_add(1);

        // Flat invariant: avg_entry must be ZERO when qty is zero.
        if self.qty.is_zero() {
            self.avg_entry = FixedI64::ZERO;
        }
    }

    /// Mark-to-market unrealized `PnL` at `current_mid`.
    ///
    /// Returns `ZERO` when the position is flat.
    #[must_use]
    pub fn unrealized_pnl(&self, current_mid: FixedI64<6>) -> FixedI64<6> {
        if self.qty.is_zero() {
            return FixedI64::ZERO;
        }
        let qty_raw = self.qty.to_raw(); // signed
        // pnl = qty * (current_mid - avg_entry)
        let price_diff = current_mid
            .checked_sub(self.avg_entry)
            .unwrap_or(FixedI64::ZERO);
        price_diff
            .checked_mul_int(qty_raw)
            .unwrap_or(FixedI64::ZERO)
    }

    /// Total `PnL` = realized + unrealized at `current_mid`.
    #[must_use]
    pub fn total_pnl(&self, current_mid: FixedI64<6>) -> FixedI64<6> {
        self.realized_pnl
            .checked_add(self.unrealized_pnl(current_mid))
            .unwrap_or(self.realized_pnl)
    }

    /// Gross notional exposure = `|qty|` * `current_mid`.
    ///
    /// Always non-negative.
    #[must_use]
    pub fn notional(&self, current_mid: FixedI64<6>) -> FixedI64<6> {
        let abs_qty = self.qty.to_raw().abs();
        current_mid
            .checked_mul_int(abs_qty)
            .unwrap_or(FixedI64::ZERO)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code — panics are acceptable")]
mod tests {
    use super::*;

    fn price(p: i64) -> FixedI64<6> {
        // Construct a price in whole units (e.g., price(100) = 100.000000)
        FixedI64::from_int(p).expect("test price fits")
    }

    fn instrument() -> InstrumentId {
        InstrumentId::from_raw(1)
    }

    #[test]
    fn new_position_is_flat() {
        let pos = Position::new(instrument());
        assert!(pos.qty.is_zero());
        assert!(pos.avg_entry.is_zero());
        assert!(pos.realized_pnl.is_zero());
        assert_eq!(pos.fill_count, 0);
    }

    #[test]
    fn buy_increases_position() {
        let mut pos = Position::new(instrument());
        pos.on_fill(Side::Bid, Lots::from_raw(10), price(100));

        assert_eq!(pos.qty.to_raw(), 10);
        assert_eq!(pos.avg_entry, price(100));
        assert_eq!(pos.fill_count, 1);
    }

    #[test]
    fn buy_twice_updates_vwap() {
        let mut pos = Position::new(instrument());
        pos.on_fill(Side::Bid, Lots::from_raw(10), price(100));
        pos.on_fill(Side::Bid, Lots::from_raw(10), price(200));

        // VWAP = (10*100 + 10*200) / 20 = 3000/20 = 150
        assert_eq!(pos.qty.to_raw(), 20);
        assert_eq!(pos.avg_entry, price(150));
        assert_eq!(pos.fill_count, 2);
    }

    #[test]
    fn sell_decreases_position() {
        let mut pos = Position::new(instrument());
        pos.on_fill(Side::Bid, Lots::from_raw(10), price(100));
        pos.on_fill(Side::Ask, Lots::from_raw(5), price(110));

        assert_eq!(pos.qty.to_raw(), 5);
        // realized_pnl = 5 * (110 - 100) = 50
        assert_eq!(pos.realized_pnl, price(50));
        assert_eq!(pos.fill_count, 2);
    }

    #[test]
    fn sell_to_flat_zeroes_avg_entry() {
        let mut pos = Position::new(instrument());
        pos.on_fill(Side::Bid, Lots::from_raw(10), price(100));
        pos.on_fill(Side::Ask, Lots::from_raw(10), price(120));

        assert!(pos.qty.is_zero());
        assert!(pos.avg_entry.is_zero());
        // realized_pnl = 10 * (120 - 100) = 200
        assert_eq!(pos.realized_pnl, price(200));
    }

    #[test]
    fn unrealized_pnl_long() {
        let mut pos = Position::new(instrument());
        pos.on_fill(Side::Bid, Lots::from_raw(10), price(100));

        // current_mid = 110: unrealized = 10 * (110 - 100) = 100
        assert_eq!(pos.unrealized_pnl(price(110)), price(100));
    }

    #[test]
    fn unrealized_pnl_short() {
        let mut pos = Position::new(instrument());
        pos.on_fill(Side::Ask, Lots::from_raw(10), price(100));

        // short 10 lots at 100, mid = 90: unrealized = -10 * (90 - 100) = 100
        assert_eq!(pos.unrealized_pnl(price(90)), price(100));
        // mid = 110: unrealized = -10 * (110 - 100) = -100
        assert_eq!(pos.unrealized_pnl(price(110)), price(-100));
    }
}
