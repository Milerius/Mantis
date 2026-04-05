//! Order book trait and implementations.

mod array_book;

pub use array_book::ArrayBook;

use mantis_events::UpdateAction;
use mantis_types::{Lots, Side, Ticks};

/// Trait for order book implementations.
///
/// `ArrayBook<N>` for bounded venues, future `VecBook` for unbounded.
pub trait OrderBook: Default {
    /// Apply a single level update.
    fn apply_delta(&mut self, price: Ticks, qty: Lots, side: Side, action: UpdateAction);

    /// Clear one side of the book.
    fn clear_side(&mut self, side: Side);

    /// Clear the entire book (both sides).
    fn clear(&mut self);

    /// Best bid price and quantity, or `None` if no bids.
    fn best_bid(&self) -> Option<(Ticks, Lots)>;

    /// Best ask price and quantity, or `None` if no asks.
    fn best_ask(&self) -> Option<(Ticks, Lots)>;

    /// Fill `buf` with the top levels for one side. Returns count written.
    fn depth(&self, side: Side, buf: &mut [(Ticks, Lots)]) -> usize;

    /// Number of non-empty price levels on one side.
    fn level_count(&self, side: Side) -> usize;

    /// Total quantity across the top `levels` on one side.
    fn total_depth(&self, side: Side, levels: usize) -> Lots;
}
