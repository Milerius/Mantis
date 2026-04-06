//! L2 queue position estimator.
//!
//! Uses the `PowerProbQueueFunc` model: cancellations are biased toward
//! the back of the queue. Parameter `prob_power_n` controls the strength of
//! that bias (2.0–3.0 recommended). Fill probability is computed with a
//! Poisson model (normal approximation for large λ, direct CDF otherwise).

use mantis_types::{InstrumentId, Lots, Side, Ticks};

/// Maximum simultaneously tracked orders for queue estimation.
pub const MAX_QUEUED_ORDERS: usize = 64;

/// Maximum instruments for per-instrument take-rate tracking.
const MAX_RATE_INSTRUMENTS: usize = 16;

/// Error returned when the queue estimator is at capacity.
#[derive(Debug, Clone, Copy)]
pub struct QueueFullError;

/// Per-order queue tracking state.
#[derive(Clone, Copy, Debug)]
pub struct QueuedOrder {
    /// Client-assigned order identifier.
    pub order_id: u64,
    /// Instrument this order is resting on.
    pub instrument_id: InstrumentId,
    /// Side of the book.
    pub side: Side,
    /// Price level in ticks.
    pub price: Ticks,
    /// Order quantity.
    pub qty: Lots,
    /// Estimated quantity ahead of us in the queue.
    pub ahead_qty: Lots,
    /// Total level size when we posted (ahead + our qty).
    pub posted_at_level_size: Lots,
}

/// Per-instrument take-rate entry.
#[derive(Clone, Copy, Debug)]
struct TakeRateEntry {
    instrument_id: InstrumentId,
    rate: f64,
}

/// L2 queue position estimator using probabilistic model.
///
/// Uses `PowerProbQueueFunc`: cancels biased toward back of queue.
/// Parameter `n` controls bias strength (2.0–3.0 recommended).
/// Calibrate from live fill data: match predicted vs actual fill times.
pub struct QueueEstimator {
    orders: [Option<QueuedOrder>; MAX_QUEUED_ORDERS],
    order_count: usize,
    take_rates_bid: [Option<TakeRateEntry>; MAX_RATE_INSTRUMENTS],
    take_rates_ask: [Option<TakeRateEntry>; MAX_RATE_INSTRUMENTS],
    /// Power function parameter. Higher = cancels more biased to back.
    prob_power_n: f64,
}

impl QueueEstimator {
    /// Create a new estimator with given power parameter.
    ///
    /// Take rates are initialised to 1.0 lots/sec per instrument as a
    /// conservative prior; they converge to observed market activity via
    /// EWMA as trades arrive.
    #[must_use]
    pub fn new(prob_power_n: f64) -> Self {
        Self {
            orders: [None; MAX_QUEUED_ORDERS],
            order_count: 0,
            take_rates_bid: [None; MAX_RATE_INSTRUMENTS],
            take_rates_ask: [None; MAX_RATE_INSTRUMENTS],
            prob_power_n,
        }
    }

    /// Register a new order at the back of the queue.
    ///
    /// `current_level_size` is the total resting quantity at `price` before
    /// our order was posted; our order goes behind all of it.
    ///
    /// # Errors
    ///
    /// Returns [`QueueFullError`] if all `MAX_QUEUED_ORDERS` slots are occupied.
    #[expect(
        clippy::too_many_arguments,
        reason = "all parameters are semantically distinct and required"
    )]
    pub fn register_order(
        &mut self,
        order_id: u64,
        instrument_id: InstrumentId,
        side: Side,
        price: Ticks,
        qty: Lots,
        current_level_size: Lots,
    ) -> Result<(), QueueFullError> {
        for slot in &mut self.orders {
            if slot.is_none() {
                *slot = Some(QueuedOrder {
                    order_id,
                    instrument_id,
                    side,
                    price,
                    qty,
                    ahead_qty: current_level_size,
                    posted_at_level_size: Lots::from_raw(
                        current_level_size.to_raw() + qty.to_raw(),
                    ),
                });
                self.order_count += 1;
                return Ok(());
            }
        }
        Err(QueueFullError)
    }

    /// Order was (partially or fully) filled — update or remove from tracking.
    ///
    /// Partial fills reduce the tracked quantity and reset `ahead_qty` to zero
    /// (we are now at the front). Full fills remove the order entirely.
    pub fn order_filled(&mut self, order_id: u64, fill_qty: Lots) {
        if let Some(order) = self.find_order_mut(order_id) {
            let remaining = order.qty.to_raw() - fill_qty.to_raw();
            if remaining <= 0 {
                // Fully filled — remove from tracking.
                self.remove_order(order_id);
            } else {
                // Partial fill — keep tracking with reduced qty.
                order.qty = Lots::from_raw(remaining);
                // After partial fill, we are at the front.
                order.ahead_qty = Lots::ZERO;
            }
        }
    }

    /// Order was cancelled — remove from tracking.
    pub fn order_cancelled(&mut self, order_id: u64) {
        self.remove_order(order_id);
    }

    /// Trade occurred at a price level — advances queue for orders at that price.
    ///
    /// Trades consume from the front of the queue, so `ahead_qty` is reduced
    /// by the trade quantity (clamped to zero). Take-rate EWMA is tracked
    /// per `(InstrumentId, Side)`.
    pub fn on_trade(&mut self, instrument_id: InstrumentId, side: Side, price: Ticks, qty: Lots) {
        // Update per-instrument take-rate EWMA (exponential moving average).
        let alpha = 0.1_f64;
        // i64 → f64: queue sizes fit well within f64 mantissa precision.
        #[expect(
            clippy::cast_precision_loss,
            reason = "queue sizes fit within f64 mantissa"
        )]
        let qty_f = qty.to_raw() as f64;

        let rates = match side {
            Side::Bid => &mut self.take_rates_bid,
            Side::Ask => &mut self.take_rates_ask,
        };
        Self::update_take_rate(rates, instrument_id, qty_f, alpha);

        // Trades consume from front of queue — reduce ahead_qty.
        for order in self.orders.iter_mut().flatten() {
            if order.instrument_id == instrument_id && order.side == side && order.price == price {
                let reduction = qty.to_raw().min(order.ahead_qty.to_raw());
                order.ahead_qty = Lots::from_raw((order.ahead_qty.to_raw() - reduction).max(0));
            }
        }
    }

    /// Level size changed — probabilistically attribute decrease to front/back.
    ///
    /// When the level shrinks (cancellations), the `PowerProbQueueFunc` model
    /// attributes a fraction of the cancels to the back of the queue.
    /// We only advance `ahead_qty` by the fraction estimated to have been
    /// cancelled from the front.
    #[expect(
        clippy::too_many_arguments,
        reason = "all parameters are semantically distinct and required"
    )]
    pub fn on_level_change(
        &mut self,
        instrument_id: InstrumentId,
        side: Side,
        price: Ticks,
        old_qty: Lots,
        new_qty: Lots,
    ) {
        if new_qty >= old_qty {
            return; // size increased — our position unchanged
        }
        let decrease = old_qty.to_raw() - new_qty.to_raw();

        for order in self.orders.iter_mut().flatten() {
            if order.instrument_id == instrument_id && order.side == side && order.price == price {
                // i64 → f64: queue sizes fit well within f64 mantissa precision.
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "queue sizes fit within f64 mantissa"
                )]
                let front = order.ahead_qty.to_raw() as f64;
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "queue sizes fit within f64 mantissa"
                )]
                let total = old_qty.to_raw() as f64;
                if total <= 0.0 {
                    continue;
                }

                // PowerProbQueueFunc: probability that a cancel comes from
                // the back = back^n / (back^n + front^n).
                let back = total - front;
                let prob_from_back = if back + front > 0.0 {
                    libm::pow(back, self.prob_power_n)
                        / (libm::pow(back, self.prob_power_n) + libm::pow(front, self.prob_power_n))
                } else {
                    0.5
                };

                // Cancels from front = decrease * (1 - prob_from_back).
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "queue sizes fit within f64 mantissa"
                )]
                let decrease_f = decrease as f64;
                let cancel_from_front = decrease_f * (1.0 - prob_from_back);
                // Truncation is intentional: fractional lots are rounded down.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "fractional lots rounded down intentionally"
                )]
                let cancel_lots = cancel_from_front as i64;
                order.ahead_qty = Lots::from_raw((order.ahead_qty.to_raw() - cancel_lots).max(0));
            }
        }
    }

    /// Estimated quantity ahead of this order in the queue.
    ///
    /// Returns `None` if the order is not tracked.
    #[must_use]
    pub fn queue_ahead(&self, order_id: u64) -> Option<Lots> {
        self.find(order_id).map(|o| o.ahead_qty)
    }

    /// Estimated fill probability using Poisson model.
    ///
    /// `P[Poisson(take_rate × time_remaining) ≥ ahead_qty + order_qty]`
    ///
    /// The threshold includes the order's own size because enough volume
    /// must arrive to consume both the queue ahead and fill our order.
    ///
    /// Uses the normal approximation for λ > 20, direct CDF otherwise.
    #[must_use]
    pub fn fill_probability(&self, order_id: u64, time_remaining_secs: f64) -> f64 {
        let Some(order) = self.find(order_id) else {
            return 0.0;
        };
        let rate = self.instrument_take_rate(order.instrument_id, order.side);
        let lambda = rate * time_remaining_secs;
        // i64 → f64: queue sizes fit well within f64 mantissa precision.
        // Include order's own size: volume must pass ahead_qty AND fill us.
        #[expect(
            clippy::cast_precision_loss,
            reason = "queue sizes fit within f64 mantissa"
        )]
        let k = (order.ahead_qty.to_raw() + order.qty.to_raw()) as f64;
        if lambda <= 0.0 {
            return 0.0;
        }

        // Poisson survival: P[X >= k].
        if lambda > 20.0 {
            // Normal approximation: (k - lambda) / sqrt(lambda).
            let z = (k - lambda) / libm::sqrt(lambda);
            0.5 * erfc(z / libm::sqrt(2.0_f64))
        } else {
            // Direct Poisson CDF via term-by-term summation.
            let mut cdf = 0.0_f64;
            let mut term = libm::exp(-lambda);
            // k is already derived from i64, so it's non-negative; truncation is safe.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "k derived from non-negative i64; truncation safe"
            )]
            #[expect(clippy::cast_sign_loss, reason = "k is non-negative by construction")]
            let k_usize = k as usize;
            for i in 0..k_usize {
                cdf += term;
                // usize → f64: loop index fits well within mantissa.
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "loop index fits within f64 mantissa"
                )]
                let denom = (i + 1) as f64;
                term *= lambda / denom;
            }
            1.0 - cdf
        }
    }

    /// Current take rate for an instrument and side (contracts/sec EWMA).
    ///
    /// Returns the conservative prior (1.0) if no trades have been observed
    /// for the given instrument.
    #[must_use]
    pub fn take_rate(&self, instrument_id: InstrumentId, side: Side) -> f64 {
        self.instrument_take_rate(instrument_id, side)
    }

    /// Lookup the per-instrument take rate, returning the default prior if
    /// no entry exists yet.
    fn instrument_take_rate(&self, instrument_id: InstrumentId, side: Side) -> f64 {
        let rates = match side {
            Side::Bid => &self.take_rates_bid,
            Side::Ask => &self.take_rates_ask,
        };
        for entry in rates.iter().flatten() {
            if entry.instrument_id == instrument_id {
                return entry.rate;
            }
        }
        // Conservative prior — no observed trades yet.
        1.0
    }

    /// Update (or insert) the EWMA take rate for an instrument.
    fn update_take_rate(
        rates: &mut [Option<TakeRateEntry>; MAX_RATE_INSTRUMENTS],
        instrument_id: InstrumentId,
        qty_f: f64,
        alpha: f64,
    ) {
        // Try to find existing entry.
        for entry in rates.iter_mut().flatten() {
            if entry.instrument_id == instrument_id {
                entry.rate = entry.rate * (1.0 - alpha) + qty_f * alpha;
                return;
            }
        }
        // Insert into first empty slot.
        for slot in rates.iter_mut() {
            if slot.is_none() {
                *slot = Some(TakeRateEntry {
                    instrument_id,
                    rate: qty_f,
                });
                return;
            }
        }
        // All slots full — overwrite the first slot (least likely to be
        // relevant if we have >16 instruments, which is unusual).
        rates[0] = Some(TakeRateEntry {
            instrument_id,
            rate: qty_f,
        });
    }

    fn find(&self, order_id: u64) -> Option<&QueuedOrder> {
        self.orders
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|o| o.order_id == order_id)
    }

    fn find_order_mut(&mut self, order_id: u64) -> Option<&mut QueuedOrder> {
        self.orders
            .iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|o| o.order_id == order_id)
    }

    fn remove_order(&mut self, order_id: u64) {
        for slot in &mut self.orders {
            if let Some(order) = slot
                && order.order_id == order_id
            {
                *slot = None;
                self.order_count = self.order_count.saturating_sub(1);
                return;
            }
        }
    }
}

impl Default for QueueEstimator {
    fn default() -> Self {
        Self::new(3.0)
    }
}

/// Complementary error function approximation (Abramowitz & Stegun 7.1.26).
///
/// Maximum error: 1.5e-7 over the real line.
fn erfc(x: f64) -> f64 {
    let abs_x = if x < 0.0 { -x } else { x };
    let t = 1.0 / (1.0 + 0.327_591_1 * abs_x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let result = poly * libm::exp(-abs_x * abs_x);
    if x >= 0.0 { result } else { 2.0 - result }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test code — panics are acceptable")]
mod tests {
    use super::*;
    use mantis_types::{InstrumentId, Lots, Side, Ticks};

    #[test]
    fn register_order_at_back_of_queue() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");
        let ahead = qe.queue_ahead(1).expect("order 1 registered");
        assert_eq!(ahead.to_raw(), 500);
    }

    #[test]
    fn trade_reduces_queue_ahead() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");
        qe.on_trade(
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(200),
        );
        let ahead = qe.queue_ahead(1).expect("order 1 registered");
        assert_eq!(ahead.to_raw(), 300);
    }

    #[test]
    fn trade_at_different_price_no_effect() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");
        qe.on_trade(
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(640),
            Lots::from_raw(200),
        );
        assert_eq!(qe.queue_ahead(1).expect("order 1 registered").to_raw(), 500,);
    }

    #[test]
    fn level_decrease_proportional() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");
        // Level shrinks from 600 to 400 (200 cancelled).
        qe.on_level_change(
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(600),
            Lots::from_raw(400),
        );
        // ahead should decrease (moved forward) but not by the full 200
        // (bias toward back means most cancels come from behind us).
        let ahead = qe.queue_ahead(1).expect("order 1 registered");
        assert!(ahead.to_raw() < 500, "ahead={}", ahead.to_raw()); // moved forward
        assert!(ahead.to_raw() > 300, "ahead={}", ahead.to_raw()); // not full amount
    }

    #[test]
    fn fill_probability_decreases_with_more_ahead() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(100),
        )
        .expect("capacity not exceeded");
        qe.register_order(
            2,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(1000),
        )
        .expect("capacity not exceeded");
        let p1 = qe.fill_probability(1, 60.0);
        let p2 = qe.fill_probability(2, 60.0);
        assert!(p1 > p2, "p1={p1}, p2={p2}"); // less ahead → higher fill prob
    }

    #[test]
    fn cancelled_order_removed() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");
        qe.order_cancelled(1);
        assert!(qe.queue_ahead(1).is_none());
    }

    #[test]
    fn partial_fill_keeps_order_tracked() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");

        // Partial fill of 40 lots.
        qe.order_filled(1, Lots::from_raw(40));
        // Order should still be tracked with reduced qty and ahead_qty = 0.
        let ahead = qe
            .queue_ahead(1)
            .expect("order still tracked after partial fill");
        assert_eq!(ahead.to_raw(), 0);
        let order = qe.find(1).expect("order still exists");
        assert_eq!(order.qty.to_raw(), 60); // 100 - 40
    }

    #[test]
    fn full_fill_removes_order() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(500),
        )
        .expect("capacity not exceeded");
        qe.order_filled(1, Lots::from_raw(100));
        assert!(
            qe.queue_ahead(1).is_none(),
            "fully filled order should be removed"
        );
    }

    #[test]
    fn per_instrument_take_rate() {
        let mut qe = QueueEstimator::new(3.0);
        // Default rate is 1.0.
        assert!((qe.take_rate(InstrumentId::from_raw(1), Side::Bid) - 1.0).abs() < f64::EPSILON);

        // Trade on instrument 1 bid.
        qe.on_trade(
            InstrumentId::from_raw(1),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(50),
        );
        let rate1 = qe.take_rate(InstrumentId::from_raw(1), Side::Bid);
        // Instrument 2 should still be at the default.
        let rate2 = qe.take_rate(InstrumentId::from_raw(2), Side::Bid);
        assert!((rate2 - 1.0).abs() < f64::EPSILON, "rate2={rate2}");
        assert!(
            (rate1 - rate2).abs() > f64::EPSILON,
            "rate1={rate1}, rate2={rate2}"
        );
    }

    #[test]
    fn fill_probability_zero_lambda_returns_zero() {
        // lambda = take_rate * time = 1.0 * 0.0 = 0 → should return 0.0
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(100),
            Lots::from_raw(10),
            Lots::from_raw(0),
        )
        .expect("capacity not exceeded");
        let p = qe.fill_probability(1, 0.0);
        assert!(p < f64::EPSILON, "expected 0.0, got {p}");
    }

    #[test]
    fn fill_probability_small_lambda_direct_cdf() {
        // lambda = 1.0 * 5.0 = 5.0 (≤ 20) — exercises direct Poisson CDF path
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(100),
            Lots::from_raw(1),
            Lots::from_raw(2),
        )
        .expect("capacity not exceeded");
        let p = qe.fill_probability(1, 5.0); // lambda = 5.0 ≤ 20
        assert!(p > 0.0 && p <= 1.0, "p={p}");
    }

    #[test]
    fn fill_probability_large_lambda_normal_approx() {
        // lambda > 20 exercises the normal approximation path via erfc
        let mut qe = QueueEstimator::new(3.0);
        // Trade many times to push the EWMA take_rate up
        for _ in 0..50 {
            qe.on_trade(
                InstrumentId::from_raw(5),
                Side::Ask,
                Ticks::from_raw(200),
                Lots::from_raw(100),
            );
        }
        qe.register_order(
            10,
            InstrumentId::from_raw(5),
            Side::Ask,
            Ticks::from_raw(200),
            Lots::from_raw(1),
            Lots::from_raw(5),
        )
        .expect("capacity not exceeded");
        // time_remaining large enough that lambda = rate * time > 20
        let p = qe.fill_probability(10, 1.0);
        assert!(p > 0.0 && p <= 1.0, "p={p}");
    }

    #[test]
    fn fill_probability_unknown_order_returns_zero() {
        let qe = QueueEstimator::new(3.0);
        let p = qe.fill_probability(999, 60.0);
        assert!(p < f64::EPSILON, "expected 0.0 for unknown order, got {p}");
    }

    #[test]
    fn on_level_change_increase_is_noop() {
        let mut qe = QueueEstimator::new(3.0);
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(100),
            Lots::from_raw(300),
        )
        .expect("capacity not exceeded");
        // Level increases — should be a no-op for queue position
        qe.on_level_change(
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(650),
            Lots::from_raw(400),
            Lots::from_raw(600),
        );
        assert_eq!(qe.queue_ahead(1).expect("order tracked").to_raw(), 300);
    }

    #[test]
    fn take_rate_unknown_instrument_returns_default() {
        let qe = QueueEstimator::new(3.0);
        // No trades recorded yet — both sides return the 1.0 prior
        let bid_rate = qe.take_rate(InstrumentId::from_raw(42), Side::Bid);
        let ask_rate = qe.take_rate(InstrumentId::from_raw(42), Side::Ask);
        assert!((bid_rate - 1.0).abs() < f64::EPSILON, "bid_rate={bid_rate}");
        assert!((ask_rate - 1.0).abs() < f64::EPSILON, "ask_rate={ask_rate}");
    }

    #[test]
    fn register_order_capacity_full_returns_error() {
        let mut qe = QueueEstimator::new(3.0);
        for i in 0..MAX_QUEUED_ORDERS {
            qe.register_order(
                i as u64,
                InstrumentId::from_raw(0),
                Side::Bid,
                Ticks::from_raw(100),
                Lots::from_raw(1),
                Lots::from_raw(0),
            )
            .expect("should fit");
        }
        let err = qe.register_order(
            MAX_QUEUED_ORDERS as u64,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(100),
            Lots::from_raw(1),
            Lots::from_raw(0),
        );
        assert!(err.is_err(), "expected QueueFullError");
    }

    #[test]
    fn default_uses_power_3() {
        // Default::default() should produce a working estimator (power = 3.0)
        let mut qe = QueueEstimator::default();
        qe.register_order(
            1,
            InstrumentId::from_raw(0),
            Side::Bid,
            Ticks::from_raw(100),
            Lots::from_raw(10),
            Lots::from_raw(50),
        )
        .expect("capacity not exceeded");
        assert_eq!(qe.queue_ahead(1).expect("tracked").to_raw(), 50);
    }

    #[test]
    fn erfc_symmetry_and_bounds() {
        // erfc(0) ≈ 1.0; erfc(large positive) ≈ 0; erfc(negative) ≈ 2 - erfc(|x|)
        let at_zero = erfc(0.0);
        assert!((at_zero - 1.0).abs() < 1e-6, "erfc(0)={at_zero}");
        let at_large = erfc(5.0);
        assert!(at_large < 1e-6, "erfc(5)={at_large}");
        let at_neg = erfc(-1.0);
        let at_pos = erfc(1.0);
        assert!((at_neg + at_pos - 2.0).abs() < 1e-6, "erfc(-x)+erfc(x)≈2");
    }

    #[test]
    fn per_instrument_take_rate_ask_side_independent() {
        let mut qe = QueueEstimator::new(3.0);
        // Trade on ask side should not affect bid rate
        qe.on_trade(
            InstrumentId::from_raw(0),
            Side::Ask,
            Ticks::from_raw(100),
            Lots::from_raw(80),
        );
        let bid_rate = qe.take_rate(InstrumentId::from_raw(0), Side::Bid);
        assert!(
            (bid_rate - 1.0).abs() < f64::EPSILON,
            "bid_rate should be default; got {bid_rate}"
        );
        let ask_rate = qe.take_rate(InstrumentId::from_raw(0), Side::Ask);
        assert!(
            (ask_rate - 1.0).abs() > f64::EPSILON,
            "ask_rate should differ from default; got {ask_rate}"
        );
    }
}
