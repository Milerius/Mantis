//! `Display` and `Debug` formatting for `FixedI64`.

use core::fmt;

use crate::FixedI64;

impl<const D: u8> fmt::Display for FixedI64<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if D == 0 {
            return write!(f, "{}", self.to_raw());
        }

        let raw = self.to_raw();
        // Use i128 unsigned abs to avoid overflow on i64::MIN.
        let abs_wide = i128::from(raw).unsigned_abs();
        // SCALE is always positive, so the sign cast is safe.
        #[expect(clippy::cast_sign_loss, reason = "SCALE is always positive")]
        let scale = Self::SCALE as u128;
        let whole = abs_wide / scale;
        let frac = abs_wide % scale;

        if raw < 0 {
            write!(f, "-{whole}.{frac:0>width$}", width = D as usize)
        } else {
            write!(f, "{whole}.{frac:0>width$}", width = D as usize)
        }
    }
}

impl<const D: u8> fmt::Debug for FixedI64<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FixedI64<{D}>(raw={}, value={self})", self.to_raw())
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::format;

    use crate::FixedI64;

    type F2 = FixedI64<2>;
    type F6 = FixedI64<6>;
    type F8 = FixedI64<8>;
    type F0 = FixedI64<0>;

    #[test]
    fn display_positive() {
        let a = F6::from_raw(1_500_000); // 1.5
        assert_eq!(format!("{a}"), "1.500000");
    }

    #[test]
    fn display_negative() {
        let a = F6::from_raw(-1_500_000);
        assert_eq!(format!("{a}"), "-1.500000");
    }

    #[test]
    fn display_zero() {
        assert_eq!(format!("{}", F6::ZERO), "0.000000");
    }

    #[test]
    fn display_zero_d2() {
        assert_eq!(format!("{}", F2::ZERO), "0.00");
    }

    #[test]
    fn display_small_d8() {
        let a = F8::from_raw(1); // 0.00000001
        assert_eq!(format!("{a}"), "0.00000001");
    }

    #[test]
    fn display_negative_small() {
        let a = F8::from_raw(-1);
        assert_eq!(format!("{a}"), "-0.00000001");
    }

    #[test]
    fn display_one() {
        assert_eq!(format!("{}", F6::ONE), "1.000000");
    }

    #[test]
    fn display_d0() {
        let a = F0::from_raw(42);
        assert_eq!(format!("{a}"), "42");
    }

    #[test]
    fn display_min() {
        // Ensure MIN doesn't panic (i64::MIN abs overflow handled via i128).
        let s = format!("{}", F6::MIN);
        assert!(s.starts_with('-'));
    }

    #[test]
    fn debug_shows_raw_and_value() {
        let a = F6::from_raw(1_500_000);
        let s = format!("{a:?}");
        assert!(s.contains("raw=1500000"));
        assert!(s.contains("value=1.500000"));
        assert!(s.contains("FixedI64<6>"));
    }
}
