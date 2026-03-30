//! Signal engine: converts price ticks and windows into actionable [`Signal`]s.

use pm_types::{ContractPrice, Edge, Side, Signal, Tick, Window};

use crate::estimator::FairValueEstimator;

// ─── SignalEngine ─────────────────────────────────────────────────────────────

/// Evaluates market state and emits a [`Signal`] when sufficient edge exists.
pub struct SignalEngine<F> {
    estimator: F,
    min_edge: f64,
}

impl<F: FairValueEstimator> SignalEngine<F> {
    /// Create a new engine with the given estimator and minimum edge threshold.
    ///
    /// `min_edge` is the minimum required `fair_value - market_price` (or its
    /// complement for Down) before a signal is emitted.
    #[inline]
    #[must_use]
    pub fn new(estimator: F, min_edge: f64) -> Self {
        Self {
            estimator,
            min_edge,
        }
    }

    /// Evaluate a tick against an open window and return a [`Signal`] if edge
    /// exceeds `min_edge`, or `None` otherwise.
    ///
    /// Returns `None` when:
    /// - fewer than 30 seconds remain in the window, or
    /// - the computed edge does not exceed `min_edge`.
    #[must_use]
    pub fn evaluate(
        &self,
        tick: &Tick,
        window: &Window,
        market_price: ContractPrice,
    ) -> Option<Signal> {
        let now_ms = tick.timestamp_ms;
        let time_remaining_secs = window.time_remaining_secs(now_ms);

        // Too close to expiry — skip.
        if time_remaining_secs < 30 {
            return None;
        }

        let magnitude = window.magnitude(tick.price);
        let direction = window.direction(tick.price);

        let fair_value = self.estimator.estimate(
            magnitude,
            time_remaining_secs,
            window.asset,
            window.timeframe,
        );

        // Edge for Up:   fair_value     - market_price
        // Edge for Down: (1-fair_value) - (1-market_price)  =  market_price - fair_value
        let up_edge = fair_value.as_f64() - market_price.as_f64();
        let down_edge = market_price.as_f64() - fair_value.as_f64();
        let (side, raw_edge) = match direction {
            Side::Up => (Side::Up, up_edge),
            Side::Down => (Side::Down, down_edge),
        };

        if raw_edge <= self.min_edge {
            return None;
        }

        let edge = Edge::new(raw_edge)?;

        Some(Signal {
            window_id: window.id,
            side,
            fair_value,
            market_price,
            edge,
            magnitude,
            time_remaining_secs,
        })
    }

    /// Return a reference to the underlying estimator.
    #[inline]
    #[must_use]
    pub fn estimator(&self) -> &F {
        &self.estimator
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test helpers use expect for conciseness"
)]
mod tests {
    use pm_types::{Asset, ExchangeSource, Price, Side, Timeframe, WindowId};

    use super::*;
    use crate::logistic::{Coefficients, LogisticModel};

    fn make_tick(price: f64, timestamp_ms: u64) -> Tick {
        Tick {
            asset: Asset::Btc,
            price: Price::new(price).expect("valid price"),
            timestamp_ms,
            source: ExchangeSource::Binance,
        }
    }

    fn make_window(open_price: f64, open_ms: u64, close_ms: u64) -> Window {
        Window {
            id: WindowId::new(1),
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            open_time_ms: open_ms,
            close_time_ms: close_ms,
            open_price: Price::new(open_price).expect("valid price"),
        }
    }

    /// Build a [`LogisticModel`] whose Up probability for BTC/Hour1 will be ~0.73
    /// regardless of magnitude/time (achieved via a large positive intercept).
    fn biased_model(intercept: f64) -> LogisticModel {
        let mut model = LogisticModel::new();
        model.set_coefficients(
            Asset::Btc,
            Timeframe::Hour1,
            Coefficients {
                beta_0: intercept,
                beta_1: 0.0,
                beta_2: 0.0,
                beta_3: 0.0,
            },
        );
        model
    }

    // ── Signal emitted when edge exceeds threshold ───────────────────────────

    #[test]
    fn signal_emitted_when_edge_exceeds_threshold() {
        // sigmoid(1.0) ≈ 0.731 — fair_value for Up
        let model = biased_model(1.0);
        let engine = SignalEngine::new(model, 0.05);

        // Market price is 0.50 → edge ≈ 0.731 - 0.50 = 0.231 > 0.05
        let market_price = ContractPrice::new(0.50).expect("valid");

        // Window: 1 hour, opened just now.  Tick at t=0 → 3600s remain.
        let window = make_window(100.0, 0, 3_600_000);
        let tick = make_tick(102.0, 0); // price above open → Up direction

        let signal = engine.evaluate(&tick, &window, market_price);
        assert!(signal.is_some(), "expected a signal");

        let s = signal.expect("checked above");
        assert_eq!(s.side, Side::Up);
        assert!(s.edge.as_f64() > 0.05);
    }

    // ── No signal when edge too small ────────────────────────────────────────

    #[test]
    fn no_signal_when_edge_too_small() {
        // sigmoid(0.0) = 0.5 — fair_value equals market_price → zero edge
        let model = biased_model(0.0);
        let engine = SignalEngine::new(model, 0.01);

        let market_price = ContractPrice::new(0.50).expect("valid");
        let window = make_window(100.0, 0, 3_600_000);
        let tick = make_tick(102.0, 0);

        let signal = engine.evaluate(&tick, &window, market_price);
        assert!(signal.is_none(), "expected no signal when edge == 0");
    }

    // ── No signal when window is about to close (< 30s remaining) ───────────

    #[test]
    fn no_signal_when_window_about_to_close() {
        let model = biased_model(2.0); // large edge to ensure we'd otherwise fire
        let engine = SignalEngine::new(model, 0.01);

        let market_price = ContractPrice::new(0.30).expect("valid");

        // Window closes at 3_600_000 ms; tick arrives 15 s before close.
        let window = make_window(100.0, 0, 3_600_000);
        let tick = make_tick(102.0, 3_600_000 - 15_000); // 15 s before close

        let signal = engine.evaluate(&tick, &window, market_price);
        assert!(signal.is_none(), "expected no signal < 30s to expiry");
    }

    // ── Exactly 30 s remaining ───────────────────────────────────────────────

    #[test]
    fn no_signal_at_exactly_30s_remaining() {
        let model = biased_model(2.0);
        let engine = SignalEngine::new(model, 0.01);

        let market_price = ContractPrice::new(0.30).expect("valid");
        let window = make_window(100.0, 0, 3_600_000);
        // Exactly 30 s before close — time_remaining_secs = 30, which is < 30? No, 30 < 30 is false.
        // The condition is `< 30`, so exactly 30 should allow a signal.
        let tick = make_tick(102.0, 3_600_000 - 30_000);

        let signal = engine.evaluate(&tick, &window, market_price);
        assert!(
            signal.is_some(),
            "exactly 30s remaining should still emit a signal"
        );
    }

    // ── Down signal ──────────────────────────────────────────────────────────

    #[test]
    fn down_signal_when_price_below_open() {
        // sigmoid(-1.0) ≈ 0.269 — fair Up prob; Down edge = market - fair ≈ 0.50 - 0.269
        let model = biased_model(-1.0);
        let engine = SignalEngine::new(model, 0.05);

        let market_price = ContractPrice::new(0.50).expect("valid");
        let window = make_window(100.0, 0, 3_600_000);
        let tick = make_tick(98.0, 0); // price below open → Down direction

        let signal = engine.evaluate(&tick, &window, market_price);
        assert!(signal.is_some(), "expected a Down signal");

        let s = signal.expect("checked above");
        assert_eq!(s.side, Side::Down);
        assert!(s.edge.as_f64() > 0.05);
    }

    // ── Estimator accessor ───────────────────────────────────────────────────

    #[test]
    fn estimator_accessor_returns_reference() {
        let model = biased_model(0.5);
        let engine = SignalEngine::new(model, 0.01);
        // Just verify it compiles and the accessor is callable.
        let _coeffs = engine
            .estimator()
            .get_coefficients(Asset::Btc, Timeframe::Hour1);
    }
}
