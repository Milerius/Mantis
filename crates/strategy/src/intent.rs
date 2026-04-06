//! Order intent types emitted by strategies for the execution layer.

use mantis_types::{InstrumentId, Lots, Side, Ticks};

/// Max intents per `on_event` call. 32 covers a full ladder reprice
/// (16 cancels + 16 posts). Strategies that need more spread across events.
pub const MAX_INTENTS_PER_TICK: usize = 32;

/// Order action type.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderAction {
    /// Post a new order.
    #[default]
    Post = 0,
    /// Cancel an existing order.
    Cancel = 1,
    /// Amend an existing order (cancel+replace on venues without native amend).
    Amend = 2,
}

/// Order intent emitted by strategy for the execution layer.
///
/// Fixed-size, `Copy`, `repr(C)` — flows through SPSC ring with zero allocation.
///
/// Field semantics by action:
/// - **Post**: `client_order_id` = new order ID, `target_order_id` = 0 (unused)
/// - **Cancel**: `target_order_id` = order to cancel, `client_order_id` = 0 (unused)
/// - **Amend**: `target_order_id` = order to replace, `client_order_id` = replacement ID
#[derive(Clone, Copy, Default, Debug)]
#[repr(C)]
pub struct OrderIntent {
    /// Instrument this intent is for.
    pub instrument_id: InstrumentId,
    /// Side of the order.
    pub side: Side,
    /// Price in ticks.
    pub price: Ticks,
    /// Quantity in lots (unsigned — order size).
    pub qty: Lots,
    /// Action to perform.
    pub action: OrderAction,
    /// Client-assigned order identifier for the new order (Post/Amend).
    pub client_order_id: u64,
    /// Target order identifier for Cancel/Amend. Zero for Post.
    pub target_order_id: u64,
    /// Strategy that generated this intent. Set by the framework, not the strategy.
    pub strategy_id: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_post() {
        let intent = OrderIntent::default();
        assert_eq!(intent.action, OrderAction::Post);
        assert_eq!(intent.strategy_id, 0);
        assert_eq!(intent.target_order_id, 0);
    }

    #[test]
    fn repr_c_is_copy() {
        let a = OrderIntent {
            instrument_id: InstrumentId::from_raw(1),
            side: Side::Bid,
            price: Ticks::from_raw(650),
            qty: Lots::from_raw(100),
            action: OrderAction::Post,
            client_order_id: 42,
            target_order_id: 0,
            strategy_id: 1,
        };
        let b = a; // Copy
        assert_eq!(a.client_order_id, b.client_order_id);
    }

    #[test]
    fn cancel_intent_has_target() {
        let intent = OrderIntent {
            action: OrderAction::Cancel,
            target_order_id: 123,
            ..Default::default()
        };
        assert_eq!(intent.action, OrderAction::Cancel);
        assert_eq!(intent.target_order_id, 123);
    }

    #[test]
    fn amend_has_both_ids() {
        let intent = OrderIntent {
            action: OrderAction::Amend,
            client_order_id: 456,
            target_order_id: 123,
            ..Default::default()
        };
        assert_eq!(intent.action, OrderAction::Amend);
        assert_eq!(intent.client_order_id, 456);
        assert_eq!(intent.target_order_id, 123);
    }
}
