//! Integration tests: cross-crate wiring for the full trading pipeline.
//!
//! These tests wire together pm-types, pm-signal, pm-risk, and pm-executor
//! using real types (no mocks) to verify the tick->signal->risk->fill pipeline.

#![expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]

use pm_executor::{PaperConfig, PaperExecutor};
use pm_risk::{RiskConfig, RiskManager};
use pm_signal::{AnyStrategy, EarlyDirectional, StrategyEngine, build_engine_from_config};
use pm_types::{
    Asset, ContractPrice, EntryDecision, MarketState, Price, Rejection, Side, StrategyId,
    Timeframe, WindowId,
    config::{StrategyConfig, default_strategies},
};

// ─── Helpers ────────────────────────────────────────────────────────────────

fn default_risk_config() -> RiskConfig {
    RiskConfig {
        max_position_usdc: 50.0,
        max_total_exposure_usdc: 200.0,
        max_daily_loss_usdc: 100.0,
        kelly_fraction: 0.25,
    }
}

fn default_paper_config() -> PaperConfig {
    PaperConfig {
        initial_balance: 1_000.0,
        slippage_bps: 10,
        max_position_usdc: 50.0,
        max_positions_per_window: 1,
    }
}

/// Build a MarketState suitable for triggering EarlyDirectional on a Min15 window.
///
/// EarlyDirectional fires when:
///   - time_elapsed_secs <= max_entry_time_secs (scaled by timeframe/900)
///   - spot_magnitude >= min_spot_magnitude
///   - direction_ask <= max_entry_price
///
/// With Min15 (900s), scale = 1.0, so effective_max = max_entry_time_secs.
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
    }
}

/// Build a MarketState suitable for triggering MomentumConfirmation on a Min15 window.
///
/// MomentumConfirmation fires when:
///   - min_entry_time_secs <= elapsed <= max_entry_time_secs (scaled)
///   - spot_magnitude >= min_spot_magnitude
///   - direction_ask <= max_entry_price
///
/// Default config: min=180, max=480 on a 900s window (scale=1.0).
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
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 1: Full tick -> signal -> risk -> fill pipeline
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn full_pipeline_early_directional_signal_to_fill() {
    // 1. Build strategy engine with EarlyDirectional only.
    let engine = StrategyEngine::from_any(vec![AnyStrategy::Early(EarlyDirectional::new(
        180,   // max_entry_time_secs
        0.001, // min_spot_magnitude
        0.58,  // max_entry_price
    ))]);

    // 2. Construct MarketState: early in window, strong move, cheap ask.
    let state = early_directional_state(1, 60);

    // 3. Strategy engine produces a signal.
    let decision = engine
        .evaluate(&state)
        .expect("EarlyDirectional should fire: elapsed=60 <= 180, mag=0.005 >= 0.001, ask=0.52 <= 0.58");
    assert_eq!(decision.side, Side::Up);
    assert_eq!(decision.strategy_id, StrategyId::EarlyDirectional);
    assert!(decision.confidence > 0.0 && decision.confidence <= 1.0);

    // 4. Risk manager approves the trade.
    let risk = RiskManager::new(default_risk_config());
    let order = risk
        .evaluate(&decision, state.window_id, state.asset, 1_000.0)
        .expect("risk manager should approve: no open positions, well within limits");
    assert_eq!(order.side, Side::Up);
    assert_eq!(order.asset, Asset::Btc);
    assert!(order.size_usdc > 0.0);

    // 5. Paper executor opens the position.
    let mut executor = PaperExecutor::new(default_paper_config());
    let fill = executor
        .try_open_position(&decision, state.window_id, state.asset, 1_000, order.size_usdc)
        .expect("paper executor should fill the order");
    assert!(fill.size_usdc > 0.0);
    assert_eq!(executor.open_position_count(), 1);
    assert!(executor.has_position_in_window(state.window_id));

    // 6. Window resolves as a win (Up outcome matches Up side).
    let balance_after_open = executor.balance();
    let pnl = executor.resolve_window(state.window_id, Side::Up, 900_000);
    assert!(
        pnl.as_f64() > 0.0,
        "winning trade should produce positive PnL"
    );
    assert!(
        executor.balance() > balance_after_open,
        "balance should increase after winning trade"
    );
    assert_eq!(executor.open_position_count(), 0);
    assert_eq!(executor.trades().len(), 1);
    assert!(executor.trades()[0].is_win());
}

#[test]
fn full_pipeline_losing_trade() {
    let engine = StrategyEngine::from_any(vec![AnyStrategy::Early(EarlyDirectional::new(
        180, 0.001, 0.58,
    ))]);

    let state = early_directional_state(1, 60);
    let decision = engine
        .evaluate(&state)
        .expect("strategy should fire");

    let risk = RiskManager::new(default_risk_config());
    let order = risk
        .evaluate(&decision, state.window_id, state.asset, 1_000.0)
        .expect("risk should approve");

    let mut executor = PaperExecutor::new(default_paper_config());
    let initial_balance = executor.balance();
    executor
        .try_open_position(&decision, state.window_id, state.asset, 1_000, order.size_usdc)
        .expect("should fill");

    // Window resolves as a loss (Down != Up).
    let pnl = executor.resolve_window(state.window_id, Side::Down, 900_000);
    assert!(pnl.as_f64() < 0.0, "losing trade should produce negative PnL");
    assert!(
        executor.balance() < initial_balance,
        "balance should be lower after a loss"
    );
    assert!(!executor.trades()[0].is_win());
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 2: Window transition lifecycle
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn window_lifecycle_open_position_then_resolve() {
    let engine = StrategyEngine::from_any(vec![AnyStrategy::Early(EarlyDirectional::new(
        180, 0.001, 0.58,
    ))]);
    let mut executor = PaperExecutor::new(default_paper_config());

    // Window 1: first tick arrives, signal fires, position opened.
    let state_w1 = early_directional_state(1, 60);
    let decision = engine.evaluate(&state_w1).expect("should fire");
    let fill = executor
        .try_open_position(&decision, state_w1.window_id, state_w1.asset, 1_000, 25.0)
        .expect("first fill should succeed");
    assert!(fill.size_usdc > 0.0);
    assert_eq!(executor.open_position_count(), 1);

    // Duplicate signal in same window is rejected by position cap.
    let second = executor.try_open_position(&decision, state_w1.window_id, state_w1.asset, 2_000, 25.0);
    assert!(
        second.is_none(),
        "second position in same window should be blocked by max_positions_per_window=1"
    );
    assert_eq!(executor.open_position_count(), 1, "still only one position");

    // Window resolves (win).
    let pnl = executor.resolve_window(state_w1.window_id, Side::Up, 900_000);
    assert!(pnl.as_f64() > 0.0);
    assert_eq!(executor.open_position_count(), 0);
    assert!(!executor.has_position_in_window(state_w1.window_id));
}

#[test]
fn multiple_windows_independent_lifecycle() {
    let engine = StrategyEngine::from_any(vec![AnyStrategy::Early(EarlyDirectional::new(
        180, 0.001, 0.58,
    ))]);
    let mut executor = PaperExecutor::new(default_paper_config());

    // Open positions in two different windows.
    let state_w1 = early_directional_state(1, 60);
    let state_w2 = early_directional_state(2, 90);

    let d1 = engine.evaluate(&state_w1).expect("fires for w1");
    let d2 = engine.evaluate(&state_w2).expect("fires for w2");

    executor
        .try_open_position(&d1, state_w1.window_id, state_w1.asset, 1_000, 25.0)
        .expect("w1 fill");
    executor
        .try_open_position(&d2, state_w2.window_id, state_w2.asset, 2_000, 25.0)
        .expect("w2 fill");
    assert_eq!(executor.open_position_count(), 2);

    // Resolve window 1 as win, window 2 as loss.
    let pnl1 = executor.resolve_window(WindowId::new(1), Side::Up, 900_000);
    assert!(pnl1.as_f64() > 0.0);
    assert_eq!(executor.open_position_count(), 1, "only w2 remains");

    let pnl2 = executor.resolve_window(WindowId::new(2), Side::Down, 1_800_000);
    assert!(pnl2.as_f64() < 0.0);
    assert_eq!(executor.open_position_count(), 0);
    assert_eq!(executor.trades().len(), 2);
}

#[test]
fn pnl_calculation_win_entry_at_half() {
    // Entry at 0.50 (after slippage ~0.501), bet Up, outcome Up.
    // Payout = size_usdc / entry. PnL = payout - size_usdc > 0.
    let mut executor = PaperExecutor::new(PaperConfig {
        initial_balance: 1_000.0,
        slippage_bps: 0, // zero slippage for clean math
        max_position_usdc: 100.0,
        max_positions_per_window: 1,
    });

    let decision = EntryDecision {
        side: Side::Up,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.8,
        strategy_id: StrategyId::EarlyDirectional,
    };

    executor
        .try_open_position(&decision, WindowId::new(1), Asset::Btc, 1_000, 50.0)
        .expect("should fill");

    // entry=0.50, size=50, contracts=50/0.50=100, payout=100, pnl=100-50=50
    let pnl = executor.resolve_window(WindowId::new(1), Side::Up, 900_000);
    assert!(
        (pnl.as_f64() - 50.0).abs() < 1e-6,
        "expected PnL ~50.0 for entry at 0.50, got {}",
        pnl.as_f64()
    );
}

#[test]
fn balance_updates_correctly_across_wins_and_losses() {
    let mut executor = PaperExecutor::new(PaperConfig {
        initial_balance: 1_000.0,
        slippage_bps: 0,
        max_position_usdc: 100.0,
        max_positions_per_window: 1,
    });

    let initial = executor.balance();

    // Window 1: entry at 0.50, win.
    let d1 = EntryDecision {
        side: Side::Up,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.8,
        strategy_id: StrategyId::EarlyDirectional,
    };
    let size1 = 50.0;
    executor
        .try_open_position(&d1, WindowId::new(1), Asset::Btc, 1_000, size1)
        .expect("fill 1");
    let pnl1 = executor.resolve_window(WindowId::new(1), Side::Up, 900_000);

    let after_win = executor.balance();
    assert!(
        (after_win - (initial + pnl1.as_f64())).abs() < 1e-6,
        "balance after win should be initial + pnl"
    );

    // Window 2: entry at 0.60, loss.
    let d2 = EntryDecision {
        side: Side::Up,
        limit_price: ContractPrice::new(0.60).expect("valid"),
        confidence: 0.8,
        strategy_id: StrategyId::EarlyDirectional,
    };
    executor
        .try_open_position(&d2, WindowId::new(2), Asset::Eth, 2_000, 40.0)
        .expect("fill 2");
    let pnl2 = executor.resolve_window(WindowId::new(2), Side::Down, 1_800_000);

    // Loss: pnl = -size_usdc (position goes to zero).
    assert!(pnl2.as_f64() < 0.0);
    let after_loss = executor.balance();
    assert!(
        (after_loss - (after_win + pnl2.as_f64())).abs() < 1e-6,
        "balance after loss should be previous_balance + pnl (negative)"
    );

    // Verify total PnL from trade records matches.
    let total_record_pnl: f64 = executor.trades().iter().map(|t| t.pnl.as_f64()).sum();
    assert!(
        (after_loss - (initial + total_record_pnl)).abs() < 1e-6,
        "final balance should equal initial + sum of all PnL"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 3: Risk manager integration
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn risk_correlation_guard_blocks_third_same_side() {
    let mut risk = RiskManager::new(default_risk_config());

    // Open two Up positions on different assets.
    risk.on_position_opened(pm_types::OpenPosition {
        window_id: WindowId::new(1),
        asset: Asset::Btc,
        side: Side::Up,
        avg_entry: ContractPrice::new(0.55).expect("valid"),
        size_usdc: 10.0,
        opened_at_ms: 0,
    });
    risk.on_position_opened(pm_types::OpenPosition {
        window_id: WindowId::new(2),
        asset: Asset::Eth,
        side: Side::Up,
        avg_entry: ContractPrice::new(0.52).expect("valid"),
        size_usdc: 10.0,
        opened_at_ms: 0,
    });

    // Third Up position on Sol should be rejected.
    let decision = EntryDecision {
        side: Side::Up,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.5,
        strategy_id: StrategyId::EarlyDirectional,
    };
    let result = risk.evaluate(&decision, WindowId::new(3), Asset::Sol, 500.0);
    assert_eq!(result, Err(Rejection::CorrelationGuard));
}

#[test]
fn risk_correlation_guard_allows_opposite_side() {
    let mut risk = RiskManager::new(default_risk_config());

    // Two Up positions open.
    risk.on_position_opened(pm_types::OpenPosition {
        window_id: WindowId::new(1),
        asset: Asset::Btc,
        side: Side::Up,
        avg_entry: ContractPrice::new(0.55).expect("valid"),
        size_usdc: 10.0,
        opened_at_ms: 0,
    });
    risk.on_position_opened(pm_types::OpenPosition {
        window_id: WindowId::new(2),
        asset: Asset::Eth,
        side: Side::Up,
        avg_entry: ContractPrice::new(0.52).expect("valid"),
        size_usdc: 10.0,
        opened_at_ms: 0,
    });

    // Down position should be allowed (different side).
    let decision = EntryDecision {
        side: Side::Down,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.5,
        strategy_id: StrategyId::EarlyDirectional,
    };
    let result = risk.evaluate(&decision, WindowId::new(3), Asset::Sol, 500.0);
    assert!(result.is_ok(), "opposite side should not trigger correlation guard");
}

#[test]
fn risk_exposure_limit_blocks_when_exceeded() {
    let mut risk = RiskManager::new(RiskConfig {
        max_position_usdc: 50.0,
        max_total_exposure_usdc: 100.0,
        max_daily_loss_usdc: 200.0,
        kelly_fraction: 0.25,
    });

    // Open a position using 90 USDC of 100 USDC limit.
    risk.on_position_opened(pm_types::OpenPosition {
        window_id: WindowId::new(1),
        asset: Asset::Btc,
        side: Side::Up,
        avg_entry: ContractPrice::new(0.55).expect("valid"),
        size_usdc: 90.0,
        opened_at_ms: 0,
    });

    // Next trade: kelly = 0.25 * 0.8 * 1000 = 200 -> capped to 50. 90 + 50 = 140 > 100.
    let decision = EntryDecision {
        side: Side::Down,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.8,
        strategy_id: StrategyId::MomentumConfirmation,
    };
    let result = risk.evaluate(&decision, WindowId::new(2), Asset::Eth, 1_000.0);
    assert_eq!(result, Err(Rejection::TotalExposureLimitBreached));
}

#[test]
fn risk_kill_switch_blocks_all_trades() {
    let mut risk = RiskManager::new(default_risk_config());
    risk.set_kill_switch(true);

    let decision = EntryDecision {
        side: Side::Up,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.9,
        strategy_id: StrategyId::EarlyDirectional,
    };
    let result = risk.evaluate(&decision, WindowId::new(1), Asset::Btc, 10_000.0);
    assert_eq!(result, Err(Rejection::KillSwitchActive));
}

#[test]
fn risk_daily_loss_triggers_kill_switch_behavior() {
    let mut risk = RiskManager::new(RiskConfig {
        max_position_usdc: 50.0,
        max_total_exposure_usdc: 200.0,
        max_daily_loss_usdc: 50.0,
        kelly_fraction: 0.25,
    });

    // Record enough losses to breach the daily limit.
    risk.on_position_closed(
        WindowId::new(1),
        pm_types::Pnl::new(-30.0).expect("valid"),
    );
    risk.on_position_closed(
        WindowId::new(2),
        pm_types::Pnl::new(-25.0).expect("valid"),
    );
    // daily_pnl = -55.0 < -50.0 limit

    let decision = EntryDecision {
        side: Side::Up,
        limit_price: ContractPrice::new(0.50).expect("valid"),
        confidence: 0.5,
        strategy_id: StrategyId::EarlyDirectional,
    };
    let result = risk.evaluate(&decision, WindowId::new(3), Asset::Btc, 500.0);
    assert_eq!(
        result,
        Err(Rejection::KillSwitchActive),
        "daily loss breach should block all trades"
    );
}

#[test]
fn risk_and_executor_wired_together() {
    // Full risk -> executor pipeline with position tracking.
    let mut risk = RiskManager::new(default_risk_config());
    let mut executor = PaperExecutor::new(default_paper_config());

    let engine = StrategyEngine::from_any(vec![AnyStrategy::Early(EarlyDirectional::new(
        180, 0.001, 0.58,
    ))]);

    let state = early_directional_state(1, 60);
    let decision = engine.evaluate(&state).expect("should fire");

    // Risk approves.
    let order = risk
        .evaluate(&decision, state.window_id, state.asset, executor.balance())
        .expect("risk should approve");
    assert!(order.size_usdc > 0.0);

    // Executor opens position using the risk-approved size.
    let fill = executor
        .try_open_position(&decision, state.window_id, state.asset, 1_000, order.size_usdc)
        .expect("should fill");

    // Track position in risk manager.
    risk.on_position_opened(pm_types::OpenPosition {
        window_id: state.window_id,
        asset: state.asset,
        side: decision.side,
        avg_entry: fill.fill_price,
        size_usdc: fill.size_usdc,
        opened_at_ms: fill.timestamp_ms,
    });
    assert_eq!(risk.open_position_count(), 1);

    // Resolve window.
    let pnl = executor.resolve_window(state.window_id, Side::Up, 900_000);
    risk.on_window_resolved(state.window_id, pnl);

    assert_eq!(risk.open_position_count(), 0);
    assert!(
        (risk.daily_pnl() - pnl.as_f64()).abs() < 1e-6,
        "risk manager daily PnL should reflect resolved trade"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// Test 4: Strategy engine with config
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn build_engine_from_config_default_strategies() {
    let strategies = default_strategies();
    let engine = build_engine_from_config(&strategies);

    // Early in window with enough magnitude: EarlyDirectional should fire.
    let state = early_directional_state(1, 60);
    let decisions = engine.evaluate_all(&state);
    assert!(
        !decisions.is_empty(),
        "default strategies should produce at least one signal on a strong early move"
    );

    // Verify EarlyDirectional is among the decisions.
    let has_early = decisions
        .iter()
        .any(|d| d.strategy_id == StrategyId::EarlyDirectional);
    assert!(has_early, "EarlyDirectional should fire: elapsed=60 <= 180, mag=0.005 >= 0.001, ask=0.52 <= 0.58");
}

#[test]
fn build_engine_early_directional_fires_in_correct_window() {
    let strategies = vec![StrategyConfig::EarlyDirectional {
        max_entry_time_secs: 120,
        min_spot_magnitude: 0.003,
        max_entry_price: 0.55,
    }];
    let engine = build_engine_from_config(&strategies);

    // Should fire: elapsed=60 <= 120, mag=0.00526 >= 0.003, ask=0.52 <= 0.55
    let state_early = early_directional_state(1, 60);
    let decision = engine.evaluate(&state_early);
    assert!(decision.is_some(), "should fire early in window");
    assert_eq!(
        decision.expect("checked above").strategy_id,
        StrategyId::EarlyDirectional
    );

    // Should NOT fire: elapsed=200 > 120
    let state_late = early_directional_state(1, 200);
    assert!(
        engine.evaluate(&state_late).is_none(),
        "should not fire when too late in window"
    );
}

#[test]
fn build_engine_momentum_fires_with_sustained_move() {
    let strategies = vec![StrategyConfig::MomentumConfirmation {
        min_entry_time_secs: 180,
        max_entry_time_secs: 480,
        min_spot_magnitude: 0.003,
        max_entry_price: 0.72,
    }];
    let engine = build_engine_from_config(&strategies);

    // Should fire: elapsed=300 in [180, 480], mag=0.00526 >= 0.003, ask=0.55 <= 0.72
    let state = momentum_state(1, 300);
    let decision = engine.evaluate(&state);
    assert!(decision.is_some(), "MomentumConfirmation should fire mid-window");
    assert_eq!(
        decision.expect("checked above").strategy_id,
        StrategyId::MomentumConfirmation
    );

    // Should NOT fire: elapsed=60 < 180
    let state_too_early = momentum_state(1, 60);
    assert!(
        engine.evaluate(&state_too_early).is_none(),
        "MomentumConfirmation should not fire too early"
    );

    // Should NOT fire: elapsed=600 > 480
    let state_too_late = momentum_state(1, 600);
    assert!(
        engine.evaluate(&state_too_late).is_none(),
        "MomentumConfirmation should not fire too late"
    );
}

#[test]
fn build_engine_empty_config_produces_no_signals() {
    let engine = build_engine_from_config(&[]);

    let state = early_directional_state(1, 60);
    let decisions = engine.evaluate_all(&state);
    assert!(
        decisions.is_empty(),
        "empty strategy config should produce zero signals"
    );
    assert!(engine.evaluate(&state).is_none());
}

#[test]
fn build_engine_multiple_strategies_highest_confidence_wins() {
    let strategies = vec![
        StrategyConfig::EarlyDirectional {
            max_entry_time_secs: 180,
            min_spot_magnitude: 0.001,
            max_entry_price: 0.58,
        },
        StrategyConfig::MomentumConfirmation {
            min_entry_time_secs: 30,
            max_entry_time_secs: 180,
            min_spot_magnitude: 0.001,
            max_entry_price: 0.72,
        },
    ];
    let engine = build_engine_from_config(&strategies);

    // At elapsed=60, both should fire. evaluate() should pick the highest confidence.
    let state = early_directional_state(1, 60);
    let all = engine.evaluate_all(&state);
    assert!(all.len() >= 1, "at least one strategy should fire");

    let best = engine.evaluate(&state);
    assert!(best.is_some(), "best-of should produce a decision");

    // If both fire, the one with higher confidence wins.
    if all.len() == 2 {
        let decisions: Vec<&EntryDecision> = all.iter().collect();
        let best = best.expect("checked above");
        assert!(
            best.confidence >= decisions[0].confidence
                && best.confidence >= decisions[1].confidence,
            "evaluate() should return the highest-confidence decision"
        );
    }
}
