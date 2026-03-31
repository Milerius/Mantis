//! Live execution module for Polymarket CLOB trading.
//!
//! Provides [`LiveStrategyInstance`], a wrapper around paper
//! [`ConcreteStrategyInstance`](pm_signal::ConcreteStrategyInstance) that places
//! real Fill-or-Kill market orders via the Polymarket CLOB when signals fire.

#![deny(unsafe_code)]

pub mod clob;
pub mod instance;
pub mod order_manager;
pub mod user_ws;

pub use clob::{
    ClobContext, GtcOrderResult, LiveFill, MarketResolutionResult, cancel_order,
    check_market_resolution, init_clob_client, place_fok_order, place_gtc_order,
    redeem_winning_position,
};
pub use instance::{LiveStrategyInstance, SharedTokenMap, TokenPair};
pub use order_manager::{FilledOrder, OrderManager, PendingOrder};
pub use user_ws::{
    UserWsEvent, UserWsEventReceiver, UserWsEventSender, run_user_ws, user_ws_channel,
};
