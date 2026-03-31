//! Integration tests: cross-crate wiring for the independent strategy instance
//! architecture.
//!
//! These tests verify `ConcreteStrategyInstance` / `build_instances_from_config`
//! using real types (no mocks) to validate the tick -> signal -> fill pipeline,
//! window dedup, resolution (win/loss), kill switch, and per-instance stats.

#![expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]

use pm_signal::{
    AnyStrategy, ConcreteStrategyInstance, EarlyDirectional, MomentumConfirmation,
    build_instances_from_config,
};
use pm_types::{
    Asset, ContractPrice, MarketState, Price, Side, StrategyInstance, Timeframe, WindowId,
    config::StrategyConfig,
};

// ─── Helpers ────────────────────────────────────────────────────────────────

fn make_ed_instance(label: &str, balance: f64) -> ConcreteStrategyInstance {
    let strategy = AnyStrategy::Early(EarlyDirectional::new(150, 0.002, 0.53));
    ConcreteStrategyInstance::new(
        label.into(),
        strategy,
        balance,
        25.0,  // max_position_usdc
        100.0, // max_exposure_usdc
        0.25,  // kelly_fraction
        50.0,  // max_daily_loss
        10,    // slippage_bps
    )
}

fn make_mc_instance(label: &str, balance: f64) -> ConcreteStrategyInstance {
    let strategy = AnyStrategy::Momentum(MomentumConfirmation::new(180, 480, 0.003, 0.72));
    ConcreteStrategyInstance::new(
        label.into(),
        strategy,
        balance,
        25.0,
        100.0,
        0.25,
        50.0,
        10,
    )
}

/// Build a MarketState suitable for triggering EarlyDirectional on a Min15 window.
fn early_directional_state(window_id: u64, elapsed: u64) -> MarketState {
    MarketState {
        asset: Asset::Btc,
        timeframe: Timeframe::Min15,
        window_id: WindowId::new(window_id),
        window_open_price: Price::new(95_000.0).expect("valid price"),
        current_spot: Price::new(95_500.0).expect("valid price"),
        spot_magnitude: 0.00526, // (95500 - 95000) / 95000
        spot_direction: Side::Up,
        time_elapsed_secs: elapsed,
        time_remaining_secs: 900_u64.saturating_sub(elapsed),
        contract_ask_up: ContractPrice::new(0.52),
        contract_ask_down: ContractPrice::new(0.50),
        contract_bid_up: ContractPrice::new(0.50),
        contract_bid_down: ContractPrice::new(0.48),
        orderbook_imbalance: None,
    }
}

/// Build a MarketState suitable for triggering MomentumConfirmation on a Min15 window.
fn momentum_state(window_id: u64, elapsed: u64) -> MarketState {
    MarketState {
        asset: Asset::Btc,
        timeframe: Timeframe::Min15,
        window_id: WindowId::new(window_id),
        window_open_price: Price::new(95_000.0).expect("valid price"),
        current_spot: Price::new(95_500.0).expect("valid price"),
        spot_magnitude: 0.00526,
        spot_direction: Side::Up,
        time_elapsed_secs: elapsed,
        time_remaining_secs: 900_u64.saturating_sub(elapsed),
        contract_ask_up: ContractPrice::new(0.55),
        contract_ask_down: ContractPrice::new(0.48),
        contract_bid_up: ContractPrice::new(0.53),
        contract_bid_down: ContractPrice::new(0.46),
        orderbook_imbalance: None,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 1: Instance opens position on signal
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn instance_opens_position_on_signal() {
    let mut inst = make_ed_instance("ED-test", 500.0);
    let state = early_directional_state(1, 60);

    let fill = inst.on_tick(&state);
    assert!(fill.is_some(), "EarlyDirectional should fire on strong early move");
    assert!(
        inst.balance() < 500.0,
        "balance should decrease after opening a position"
    );

    let fill = fill.expect("checked above");
    assert_eq!(fill.side, Side::Up);
    assert!(fill.size_usdc > 0.0);
    assert!(fill.fill_price > 0.0 && fill.fill_price < 1.0);
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 2: Instance blocks duplicate in same window
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn instance_blocks_duplicate_in_same_window() {
    let mut inst = make_ed_instance("ED-test", 500.0);
    let state = early_directional_state(1, 60);

    let fill1 = inst.on_tick(&state);
    assert!(fill1.is_some(), "first tick should open a position");

    let fill2 = inst.on_tick(&state);
    assert!(
        fill2.is_none(),
        "second tick in same window should be blocked by dedup"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 3: Instance resolves win correctly (balance increases)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn instance_resolves_win_correctly() {
    let mut inst = make_ed_instance("ED-test", 500.0);
    let initial_balance = inst.balance();
    let state = early_directional_state(1, 60);

    let _fill = inst.on_tick(&state).expect("should open position");

    // Resolve as win (Up outcome matches Up side)
    let trades = inst.on_window_close(WindowId::new(1), Side::Up, 900_000);
    assert_eq!(trades.len(), 1);
    assert!(trades[0].pnl.as_f64() > 0.0, "winning trade should have positive PnL");
    assert!(
        inst.balance() > initial_balance,
        "balance should increase after winning trade"
    );
    assert_eq!(inst.stats().wins, 1);
    assert_eq!(inst.stats().losses, 0);
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 4: Instance resolves loss correctly (balance decreases)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn instance_resolves_loss_correctly() {
    let mut inst = make_ed_instance("ED-test", 500.0);
    let initial_balance = inst.balance();
    let state = early_directional_state(1, 60);

    let _fill = inst.on_tick(&state).expect("should open position");

    // Resolve as loss (Down != Up)
    let trades = inst.on_window_close(WindowId::new(1), Side::Down, 900_000);
    assert_eq!(trades.len(), 1);
    assert!(trades[0].pnl.as_f64() < 0.0, "losing trade should have negative PnL");
    assert!(
        inst.balance() < initial_balance,
        "balance should decrease after losing trade"
    );
    assert_eq!(inst.stats().wins, 0);
    assert_eq!(inst.stats().losses, 1);
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 5: Multiple instances are independent
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn multiple_instances_are_independent() {
    let mut ed = make_ed_instance("ED-test", 250.0);
    let mut mc = make_mc_instance("MC-test", 250.0);

    // Same window: ED fires early, MC fires in its time range
    let state_early = early_directional_state(1, 60);
    let state_mid = momentum_state(1, 300);

    let ed_fill = ed.on_tick(&state_early);
    assert!(ed_fill.is_some(), "ED should fire early in window");

    let mc_fill = mc.on_tick(&state_mid);
    assert!(
        mc_fill.is_some(),
        "MC should independently fire in same window — instances are separate"
    );

    // Both can resolve independently
    let ed_trades = ed.on_window_close(WindowId::new(1), Side::Up, 900_000);
    let mc_trades = mc.on_window_close(WindowId::new(1), Side::Up, 900_000);

    assert_eq!(ed_trades.len(), 1);
    assert_eq!(mc_trades.len(), 1);

    // Their balances are independent
    assert!(
        (ed.balance() - mc.balance()).abs() > f64::EPSILON,
        "instances should have different balances (different strategies, different sizing)"
    );
}

#[test]
fn two_ed_instances_can_both_fill_same_window() {
    // Two separate ED instances with different configs can both trade the same window
    let mut ed_tight = make_ed_instance("ED-tight", 125.0);
    let strategy_loose = AnyStrategy::Early(EarlyDirectional::new(200, 0.001, 0.58));
    let mut ed_loose = ConcreteStrategyInstance::new(
        "ED-loose".into(),
        strategy_loose,
        125.0,
        25.0,
        100.0,
        0.25,
        50.0,
        10,
    );

    let state = early_directional_state(1, 60);

    let fill1 = ed_tight.on_tick(&state);
    let fill2 = ed_loose.on_tick(&state);

    assert!(fill1.is_some(), "ED-tight should fire");
    assert!(fill2.is_some(), "ED-loose should independently fire on same window");
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 6: Kill switch triggers on daily loss
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn kill_switch_triggers_on_daily_loss() {
    // Small max_daily_loss to trigger quickly
    let strategy = AnyStrategy::Early(EarlyDirectional::new(150, 0.002, 0.53));
    let mut inst = ConcreteStrategyInstance::new(
        "ED-fragile".into(),
        strategy,
        500.0,
        25.0,
        100.0,
        0.25,
        10.0, // very low max_daily_loss — triggers quickly
        10,
    );

    // Repeatedly open and lose positions until kill switch triggers
    let mut blocked = false;
    for i in 1..=50u64 {
        let mut state = early_directional_state(i, 60);
        state.window_id = WindowId::new(i);
        // Use different assets/timeframes to avoid dedup
        state.asset = match i % 4 {
            0 => Asset::Btc,
            1 => Asset::Eth,
            2 => Asset::Sol,
            _ => Asset::Xrp,
        };
        state.timeframe = match (i / 4) % 4 {
            0 => Timeframe::Min5,
            1 => Timeframe::Min15,
            2 => Timeframe::Hour1,
            _ => Timeframe::Hour4,
        };

        if inst.on_tick(&state).is_some() {
            inst.on_window_close(state.window_id, Side::Down, i * 900_000);
        } else {
            // If we can't open, kill switch is likely active
            blocked = true;
            break;
        }
    }

    assert!(
        blocked,
        "kill switch should eventually block trades after enough daily losses"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 7: Per-instance stats track correctly
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn per_instance_stats_track_correctly() {
    let mut inst = make_ed_instance("ED-stats", 500.0);

    // Trade 1: Win
    let state1 = early_directional_state(1, 60);
    inst.on_tick(&state1);
    inst.on_window_close(WindowId::new(1), Side::Up, 900_000);

    assert_eq!(inst.stats().wins, 1);
    assert_eq!(inst.stats().losses, 0);
    assert!(inst.stats().realized_pnl > 0.0);
    assert!(inst.stats().biggest_win > 0.0);

    // Trade 2: Loss (different asset to avoid dedup)
    let mut state2 = early_directional_state(2, 60);
    state2.asset = Asset::Eth;
    state2.window_id = WindowId::new(2);
    inst.on_tick(&state2);
    inst.on_window_close(WindowId::new(2), Side::Down, 1_800_000);

    assert_eq!(inst.stats().wins, 1);
    assert_eq!(inst.stats().losses, 1);
    assert!((inst.stats().win_rate() - 50.0).abs() < 1e-6);
    assert!(inst.stats().biggest_loss < 0.0, "biggest_loss should be negative");
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 8: build_instances_from_config produces working instances
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn build_instances_from_config_produces_correct_count() {
    let strategies = vec![
        StrategyConfig::EarlyDirectional {
            label: "ED-tight".into(),
            mode: String::new(),
            max_entry_time_secs: 150,
            min_spot_magnitude: 0.002,
            max_entry_price: 0.53,
            balance: 125.0,
            max_position_usdc: 25.0,
            max_exposure_usdc: 100.0,
            kelly_fraction: 0.25,
            max_daily_loss: 50.0,
            slippage_bps: 10,
            order_mode: "fok".into(),
            gtc_timeout_secs: 120,
        },
        StrategyConfig::MomentumConfirmation {
            label: "MC-tight".into(),
            mode: String::new(),
            min_entry_time_secs: 180,
            max_entry_time_secs: 480,
            min_spot_magnitude: 0.003,
            max_entry_price: 0.72,
            balance: 125.0,
            max_position_usdc: 25.0,
            max_exposure_usdc: 100.0,
            kelly_fraction: 0.25,
            max_daily_loss: 50.0,
            slippage_bps: 10,
            order_mode: "fok".into(),
            gtc_timeout_secs: 120,
        },
    ];

    let instances = build_instances_from_config(&strategies);
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[0].label(), "ED-tight");
    assert_eq!(instances[1].label(), "MC-tight");
    assert!((instances[0].balance() - 125.0).abs() < f64::EPSILON);
    assert!((instances[1].balance() - 125.0).abs() < f64::EPSILON);
}

#[test]
fn build_instances_each_fires_on_appropriate_signal() {
    let strategies = vec![
        StrategyConfig::EarlyDirectional {
            label: "ED".into(),
            mode: String::new(),
            max_entry_time_secs: 150,
            min_spot_magnitude: 0.002,
            max_entry_price: 0.53,
            balance: 125.0,
            max_position_usdc: 25.0,
            max_exposure_usdc: 100.0,
            kelly_fraction: 0.25,
            max_daily_loss: 50.0,
            slippage_bps: 10,
            order_mode: "fok".into(),
            gtc_timeout_secs: 120,
        },
        StrategyConfig::MomentumConfirmation {
            label: "MC".into(),
            mode: String::new(),
            min_entry_time_secs: 180,
            max_entry_time_secs: 480,
            min_spot_magnitude: 0.003,
            max_entry_price: 0.72,
            balance: 125.0,
            max_position_usdc: 25.0,
            max_exposure_usdc: 100.0,
            kelly_fraction: 0.25,
            max_daily_loss: 50.0,
            slippage_bps: 10,
            order_mode: "fok".into(),
            gtc_timeout_secs: 120,
        },
    ];

    let mut instances = build_instances_from_config(&strategies);

    // Early tick: only ED fires
    let state_early = early_directional_state(1, 60);
    let ed_fill = instances[0].on_tick(&state_early);
    let mc_fill = instances[1].on_tick(&state_early);
    assert!(ed_fill.is_some(), "ED should fire at elapsed=60");
    assert!(mc_fill.is_none(), "MC should not fire at elapsed=60 (too early)");

    // Mid-window tick on different window: only MC fires
    let state_mid = momentum_state(2, 300);
    // ED won't fire because elapsed=300 > 150
    let mut ed_state_mid = state_mid.clone();
    ed_state_mid.window_id = WindowId::new(2);
    let ed_fill2 = instances[0].on_tick(&ed_state_mid);
    let mc_fill2 = instances[1].on_tick(&state_mid);
    assert!(ed_fill2.is_none(), "ED should not fire at elapsed=300 (too late)");
    assert!(mc_fill2.is_some(), "MC should fire at elapsed=300");
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 9: Balance math is correct across win/loss cycle
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn balance_math_across_win_loss_cycle() {
    let strategy = AnyStrategy::Early(EarlyDirectional::new(150, 0.002, 0.53));
    let mut inst = ConcreteStrategyInstance::new(
        "ED-math".into(),
        strategy,
        1000.0, // large balance for cleaner math
        25.0,
        100.0,
        0.25,
        200.0,
        0, // zero slippage for clean math
    );

    let initial = inst.balance();

    // Win: entry at 0.52, payout = size/0.52
    let state1 = early_directional_state(1, 60);
    let _fill1 = inst.on_tick(&state1).expect("should fill");

    let trades1 = inst.on_window_close(WindowId::new(1), Side::Up, 900_000);
    let pnl1 = trades1[0].pnl.as_f64();
    assert!(pnl1 > 0.0);

    let after_win = inst.balance();
    assert!(
        (after_win - (initial + pnl1)).abs() < 1e-6,
        "balance = initial + pnl after first trade: {} vs {}",
        after_win,
        initial + pnl1
    );

    // Loss: entry at 0.52, total loss = -size
    let mut state2 = early_directional_state(2, 60);
    state2.asset = Asset::Eth;
    state2.window_id = WindowId::new(2);
    let fill2 = inst.on_tick(&state2).expect("should fill");
    let size2 = fill2.size_usdc;

    let trades2 = inst.on_window_close(WindowId::new(2), Side::Down, 1_800_000);
    let pnl2 = trades2[0].pnl.as_f64();
    assert!((pnl2 + size2).abs() < 1e-6, "loss should equal -size_usdc");

    let after_loss = inst.balance();
    assert!(
        (after_loss - (after_win + pnl2)).abs() < 1e-6,
        "balance after loss should be previous + pnl"
    );

    // Total PnL from stats matches
    let total_pnl = inst.stats().realized_pnl;
    assert!(
        (after_loss - (initial + total_pnl)).abs() < 1e-6,
        "final balance should equal initial + total realized PnL"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 10: Empty config produces no instances
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn build_instances_from_empty_config() {
    let instances = build_instances_from_config(&[]);
    assert!(instances.is_empty());
}
