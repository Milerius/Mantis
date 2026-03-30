//! Price-related newtypes for Polymarket trading.
//!
//! All types wrap `f64` and validate on construction. Construction returns
//! `None` for non-finite inputs or out-of-range values. The inner value is
//! always a valid, finite `f64` satisfying the stated invariants.

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

// ─── Price ───────────────────────────────────────────────────────────────────

/// A non-negative, finite price in USD (or any base currency).
///
/// Invariant: `inner >= 0.0` and `inner.is_finite()`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct Price(f64);

impl Price {
    /// Construct a [`Price`] from a raw `f64`.
    ///
    /// Returns `None` if `value` is not finite or is negative.
    #[inline]
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() && value >= 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Return the raw `f64` value.
    #[inline]
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        self.0
    }
}

impl core::fmt::Display for Price {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── ContractPrice ───────────────────────────────────────────────────────────

/// A Polymarket binary contract price in `[0.0, 1.0]`.
///
/// Represents the probability / market price of a YES outcome.
/// Invariant: `0.0 <= inner <= 1.0` and `inner.is_finite()`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct ContractPrice(f64);

impl ContractPrice {
    /// Construct a [`ContractPrice`] from a raw `f64`.
    ///
    /// Returns `None` if `value` is outside `[0.0, 1.0]` or is not finite.
    #[inline]
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Return the raw `f64` value.
    #[inline]
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        self.0
    }
}

impl core::fmt::Display for ContractPrice {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── Edge ────────────────────────────────────────────────────────────────────

/// The pricing edge: `fair_value - market_price`.
///
/// May be negative (market is expensive) or positive (market is cheap).
/// Invariant: `inner.is_finite()`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct Edge(f64);

impl Edge {
    /// Construct an [`Edge`] from a raw `f64`.
    ///
    /// Returns `None` if `value` is not finite (NaN or ±infinity).
    #[inline]
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Return the raw `f64` value.
    #[inline]
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        self.0
    }
}

impl core::fmt::Display for Edge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── Pnl ─────────────────────────────────────────────────────────────────────

/// Realised or unrealised profit-and-loss in USD.
///
/// May be negative (loss). Invariant: `inner.is_finite()`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct Pnl(f64);

impl Pnl {
    /// Zero `PnL` — no gain, no loss.
    pub const ZERO: Self = Self(0.0);

    /// Construct a [`Pnl`] from a raw `f64`.
    ///
    /// Returns `None` if `value` is not finite (NaN or ±infinity).
    #[inline]
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Return the raw `f64` value.
    #[inline]
    #[must_use]
    pub const fn as_f64(self) -> f64 {
        self.0
    }
}

impl core::fmt::Display for Pnl {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;
    use std::string::ToString;

    use super::*;

    // Price tests

    #[test]
    fn price_rejects_negative() {
        assert!(Price::new(-0.01).is_none());
        assert!(Price::new(-1000.0).is_none());
    }

    #[test]
    fn price_rejects_nan() {
        assert!(Price::new(f64::NAN).is_none());
    }

    #[test]
    fn price_rejects_infinity() {
        assert!(Price::new(f64::INFINITY).is_none());
        assert!(Price::new(f64::NEG_INFINITY).is_none());
    }

    #[test]
    fn price_accepts_zero() {
        let p = Price::new(0.0).expect("zero price should be valid");
        assert_eq!(p.as_f64(), 0.0);
    }

    #[test]
    fn price_roundtrip() {
        let value = 42_500.75_f64;
        let p = Price::new(value).expect("positive finite price should be valid");
        assert_eq!(p.as_f64(), value);
        assert_eq!(p.to_string(), value.to_string());
    }

    // ContractPrice tests

    #[test]
    fn contract_price_rejects_below_zero() {
        assert!(ContractPrice::new(-0.01).is_none());
    }

    #[test]
    fn contract_price_rejects_above_one() {
        assert!(ContractPrice::new(1.000_001).is_none());
    }

    #[test]
    fn contract_price_rejects_nan() {
        assert!(ContractPrice::new(f64::NAN).is_none());
    }

    #[test]
    fn contract_price_boundary_zero() {
        let cp = ContractPrice::new(0.0).expect("zero is valid contract price");
        assert_eq!(cp.as_f64(), 0.0);
    }

    #[test]
    fn contract_price_boundary_one() {
        let cp = ContractPrice::new(1.0).expect("one is valid contract price");
        assert_eq!(cp.as_f64(), 1.0);
    }

    #[test]
    fn contract_price_roundtrip() {
        let value = 0.65_f64;
        let cp = ContractPrice::new(value).expect("0.65 should be valid");
        assert_eq!(cp.as_f64(), value);
        assert_eq!(cp.to_string(), value.to_string());
    }

    // Edge tests

    #[test]
    fn edge_accepts_negative() {
        let e = Edge::new(-0.05).expect("negative edge should be valid");
        assert_eq!(e.as_f64(), -0.05);
    }

    #[test]
    fn edge_accepts_positive() {
        let e = Edge::new(0.08).expect("positive edge should be valid");
        assert_eq!(e.as_f64(), 0.08);
    }

    #[test]
    fn edge_accepts_zero() {
        let e = Edge::new(0.0).expect("zero edge should be valid");
        assert_eq!(e.as_f64(), 0.0);
    }

    #[test]
    fn edge_rejects_nan() {
        assert!(Edge::new(f64::NAN).is_none());
    }

    #[test]
    fn edge_rejects_infinity() {
        assert!(Edge::new(f64::INFINITY).is_none());
    }

    // Pnl tests

    #[test]
    fn pnl_zero_const() {
        assert_eq!(Pnl::ZERO.as_f64(), 0.0);
    }

    #[test]
    fn pnl_accepts_negative() {
        let p = Pnl::new(-25.50).expect("negative pnl should be valid");
        assert_eq!(p.as_f64(), -25.50);
    }

    #[test]
    fn pnl_accepts_positive() {
        let p = Pnl::new(100.0).expect("positive pnl should be valid");
        assert_eq!(p.as_f64(), 100.0);
    }

    #[test]
    fn pnl_rejects_nan() {
        assert!(Pnl::new(f64::NAN).is_none());
    }

    #[test]
    fn pnl_rejects_infinity() {
        assert!(Pnl::new(f64::NEG_INFINITY).is_none());
    }

    #[test]
    fn pnl_roundtrip() {
        let value = -7.25_f64;
        let p = Pnl::new(value).expect("finite pnl should be valid");
        assert_eq!(p.as_f64(), value);
        assert_eq!(p.to_string(), value.to_string());
    }
}
