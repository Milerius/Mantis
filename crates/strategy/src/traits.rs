//! The core Strategy trait.

use mantis_events::HotEvent;
use crate::intent::{OrderIntent, MAX_INTENTS_PER_TICK};

/// Strategy is a self-contained event-driven state machine.
///
/// No generics on the trait — concrete strategies store their own
/// `StrategyContext<B, MAX>` internally. This keeps the trait simple
/// and allows enum dispatch without generic pain.
///
/// # Associated Constants
///
/// - `STRATEGY_ID`: unique identifier (0-255), encoded in `client_order_id`
///   high bits to route fills back. The framework writes this into
///   `OrderIntent.strategy_id` after `on_event` — strategies never set it.
/// - `NAME`: human-readable name for logging and dashboard.
///
/// # Dispatch
///
/// Bot binaries use enum dispatch (not `dyn`):
/// ```ignore
/// enum ActiveStrategies {
///     LadderMM(LadderMMStrategy),
///     Momentum(MomentumStrategy),
/// }
/// impl Strategy for ActiveStrategies { /* match self */ }
/// ```
///
/// # Replay
///
/// Feed the same `HotEvent` tape → get identical intents. The strategy
/// is a pure state machine with no external dependencies.
pub trait Strategy {
    /// Unique identifier for this strategy instance (0-255).
    const STRATEGY_ID: u8;

    /// Human-readable name for logging and dashboard.
    const NAME: &'static str;

    /// Process one event. Returns number of intents written to buffer.
    ///
    /// Called for BOTH market data events (`BookDelta`, `Trade`, `TopOfBook`)
    /// AND account events (`Fill`, `OrderAck`, `OrderReject`).
    ///
    /// The framework sets `intent.strategy_id = Self::STRATEGY_ID` after
    /// this method returns — strategies should NOT set it.
    fn on_event(
        &mut self,
        event: &HotEvent,
        intents: &mut [OrderIntent; MAX_INTENTS_PER_TICK],
    ) -> usize;
}
