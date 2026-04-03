//! Instrument metadata for tick/lot conversions.

use mantis_fixed::FixedI64;

use crate::{Lots, Ticks};

/// Instrument metadata defining tick and lot sizes for price/quantity conversion.
///
/// Both `tick_size` and `lot_size` must be positive. Use [`InstrumentMeta::new`]
/// to construct with validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstrumentMeta<const D: u8> {
    tick_size: FixedI64<D>,
    lot_size: FixedI64<D>,
}

/// Error returned when constructing an [`InstrumentMeta`] with invalid parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstrumentMetaError {
    /// The tick size must be strictly positive.
    InvalidTickSize,
    /// The lot size must be strictly positive.
    InvalidLotSize,
}

impl core::fmt::Display for InstrumentMetaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidTickSize => write!(f, "tick size must be strictly positive"),
            Self::InvalidLotSize => write!(f, "lot size must be strictly positive"),
        }
    }
}

impl<const D: u8> InstrumentMeta<D> {
    /// Construct a new `InstrumentMeta`, validating that both sizes are positive.
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentMetaError::InvalidTickSize`] if `tick_size` is zero or negative.
    /// Returns [`InstrumentMetaError::InvalidLotSize`] if `lot_size` is zero or negative.
    pub const fn new(
        tick_size: FixedI64<D>,
        lot_size: FixedI64<D>,
    ) -> Result<Self, InstrumentMetaError> {
        if !tick_size.is_positive() {
            return Err(InstrumentMetaError::InvalidTickSize);
        }
        if !lot_size.is_positive() {
            return Err(InstrumentMetaError::InvalidLotSize);
        }
        Ok(Self {
            tick_size,
            lot_size,
        })
    }

    /// The tick size for this instrument.
    #[must_use]
    pub const fn tick_size(&self) -> FixedI64<D> {
        self.tick_size
    }

    /// The lot size for this instrument.
    #[must_use]
    pub const fn lot_size(&self) -> FixedI64<D> {
        self.lot_size
    }

    /// Convert a fixed-point price to ticks by dividing raw values (truncating toward zero).
    ///
    /// This is an integer division of the raw representations:
    /// `ticks = price.raw / tick_size.raw`.
    ///
    /// Returns `None` if the tick size raw value is zero (should not happen
    /// given the construction invariant, but checked defensively).
    #[must_use]
    pub const fn price_to_ticks(&self, price: FixedI64<D>) -> Option<Ticks> {
        let divisor = self.tick_size.to_raw();
        if divisor == 0 {
            return None;
        }
        Some(Ticks::from_raw(price.to_raw() / divisor))
    }

    /// Convert ticks back to a fixed-point price by multiplying tick size by tick count.
    ///
    /// Returns `None` on overflow.
    #[must_use]
    pub const fn ticks_to_price(&self, ticks: Ticks) -> Option<FixedI64<D>> {
        self.tick_size.checked_mul_int(ticks.to_raw())
    }

    /// Convert a fixed-point quantity to lots by dividing raw values (truncating toward zero).
    ///
    /// Returns `None` if the lot size raw value is zero.
    #[must_use]
    pub const fn qty_to_lots(&self, qty: FixedI64<D>) -> Option<Lots> {
        let divisor = self.lot_size.to_raw();
        if divisor == 0 {
            return None;
        }
        Some(Lots::from_raw(qty.to_raw() / divisor))
    }

    /// Convert lots back to a fixed-point quantity by multiplying lot size by lot count.
    ///
    /// Returns `None` on overflow.
    #[must_use]
    pub const fn lots_to_qty(&self, lots: Lots) -> Option<FixedI64<D>> {
        self.lot_size.checked_mul_int(lots.to_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_zero_tick_size() {
        let result = InstrumentMeta::<6>::new(FixedI64::ZERO, FixedI64::from_raw(1_000));
        assert_eq!(result, Err(InstrumentMetaError::InvalidTickSize));
    }

    #[test]
    fn reject_negative_tick_size() {
        let result = InstrumentMeta::<6>::new(FixedI64::from_raw(-1), FixedI64::from_raw(1_000));
        assert_eq!(result, Err(InstrumentMetaError::InvalidTickSize));
    }

    #[test]
    fn reject_zero_lot_size() {
        let result = InstrumentMeta::<6>::new(FixedI64::from_raw(1_000), FixedI64::ZERO);
        assert_eq!(result, Err(InstrumentMetaError::InvalidLotSize));
    }

    #[test]
    fn reject_negative_lot_size() {
        let result = InstrumentMeta::<6>::new(FixedI64::from_raw(1_000), FixedI64::from_raw(-1));
        assert_eq!(result, Err(InstrumentMetaError::InvalidLotSize));
    }

    #[test]
    fn price_to_ticks_basic() {
        // tick_size = 0.01 (raw 10_000 at D=6), price = 1.50 (raw 1_500_000)
        // 1.50 / 0.01 = 150 ticks
        let meta = InstrumentMeta::<6>::new(FixedI64::from_raw(10_000), FixedI64::from_raw(1_000));
        let meta = match meta {
            Ok(m) => m,
            Err(_) => return,
        };
        let ticks = meta.price_to_ticks(FixedI64::from_raw(1_500_000));
        assert_eq!(ticks.map(Ticks::to_raw), Some(150));
    }

    #[test]
    fn ticks_to_price_basic() {
        // tick_size = 0.01, 150 ticks -> 1.50
        let meta = InstrumentMeta::<6>::new(FixedI64::from_raw(10_000), FixedI64::from_raw(1_000));
        let meta = match meta {
            Ok(m) => m,
            Err(_) => return,
        };
        let price = meta.ticks_to_price(Ticks::from_raw(150));
        assert_eq!(price.map(FixedI64::to_raw), Some(1_500_000));
    }

    #[test]
    fn price_roundtrip() {
        let meta = InstrumentMeta::<6>::new(
            FixedI64::from_raw(10_000), // tick_size = 0.01
            FixedI64::from_raw(1_000),
        );
        let meta = match meta {
            Ok(m) => m,
            Err(_) => return,
        };
        let original = FixedI64::<6>::from_raw(1_500_000); // 1.50
        let ticks = meta.price_to_ticks(original);
        let Some(ticks) = ticks else { return };
        let recovered = meta.ticks_to_price(ticks);
        assert_eq!(recovered.map(FixedI64::to_raw), Some(original.to_raw()));
    }

    #[test]
    fn qty_to_lots_basic() {
        // lot_size = 0.001 (raw 1_000 at D=6), qty = 2.500 (raw 2_500_000)
        // 2.500 / 0.001 = 2500 lots
        let meta = InstrumentMeta::<6>::new(FixedI64::from_raw(10_000), FixedI64::from_raw(1_000));
        let meta = match meta {
            Ok(m) => m,
            Err(_) => return,
        };
        let lots = meta.qty_to_lots(FixedI64::from_raw(2_500_000));
        assert_eq!(lots.map(Lots::to_raw), Some(2_500));
    }

    #[test]
    fn lots_roundtrip() {
        let meta = InstrumentMeta::<6>::new(
            FixedI64::from_raw(10_000),
            FixedI64::from_raw(1_000), // lot_size = 0.001
        );
        let meta = match meta {
            Ok(m) => m,
            Err(_) => return,
        };
        let original = FixedI64::<6>::from_raw(2_500_000); // 2.500
        let lots = meta.qty_to_lots(original);
        let Some(lots) = lots else { return };
        let recovered = meta.lots_to_qty(lots);
        assert_eq!(recovered.map(FixedI64::to_raw), Some(original.to_raw()));
    }
}
