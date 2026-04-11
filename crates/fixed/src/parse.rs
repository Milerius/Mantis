//! Decimal string parsing for `FixedI64`.
//!
//! Single-pass, i64-only parser optimized for HFT price strings.
//! No i128 widening, no multi-scan, POW10 lookup for zero-padding.

use core::fmt;

use crate::FixedI64;
use mantis_platform::numerics::POW10_I64;

/// Error returned when parsing a decimal string into `FixedI64`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseFixedError {
    /// The input is not a valid decimal number (empty, non-digit characters,
    /// exponent notation, double dots, etc.).
    InvalidFormat,
    /// The parsed value does not fit in `i64`.
    Overflow,
    /// The fractional part has more digits than the scale allows.
    ExcessPrecision,
}

impl fmt::Display for ParseFixedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid decimal format"),
            Self::Overflow => write!(f, "value overflows i64 at this scale"),
            Self::ExcessPrecision => write!(f, "fractional digits exceed scale D"),
        }
    }
}

/// Single-pass decimal accumulator state.
///
/// Walks the byte slice once, accumulating both whole and fractional
/// parts into a single `i64` mantissa. Records position of the dot
/// (if any) to compute the fractional digit count at the end.
///
/// # Why i64 instead of i128
///
/// Price strings from venue feeds are at most ~18 digits total
/// (e.g., `"999999999.99999999"`). The maximum value at D=8 is
/// `999_999_999 * 10^8 + 99_999_999 = 99_999_999_999_999_999`
/// which fits in i64 (max 9.2e18). We only need i128 for extreme
/// edge cases (>18 total digits), which we reject as Overflow.
struct Accumulator {
    mantissa: i64,
    total_digits: u8,
    frac_digits: u8,
    saw_dot: bool,
    saw_any_digit: bool,
}

impl Accumulator {
    const fn new() -> Self {
        Self {
            mantissa: 0,
            total_digits: 0,
            frac_digits: 0,
            saw_dot: false,
            saw_any_digit: false,
        }
    }

    /// Feed one byte. Returns Err on invalid/overflow.
    ///
    /// Accumulates in NEGATIVE space (`mantissa` is always <= 0) to handle
    /// `i64::MIN` correctly. `|i64::MIN| > i64::MAX`, so accumulating positive
    /// then negating would overflow for MIN. Instead, we accumulate as
    /// `mantissa = mantissa * 10 - digit` and negate at the end if positive.
    #[inline(always)]
    const fn feed(mut self, byte: u8) -> Result<Self, ParseFixedError> {
        match byte {
            b'0'..=b'9' => {
                let digit = (byte - b'0') as i64;

                // Skip leading zeros — they don't contribute to overflow risk
                if digit == 0 && self.mantissa == 0 {
                    // Still track fractional position
                    if self.saw_dot {
                        self.frac_digits += 1;
                    }
                    self.saw_any_digit = true;
                    return Ok(self);
                }

                self.total_digits += 1;

                if self.total_digits > 19 {
                    return Err(ParseFixedError::Overflow);
                }

                // acc = acc * 10 - digit (accumulate in negative space)
                if self.total_digits <= 18 {
                    self.mantissa = self.mantissa * 10 - digit;
                } else {
                    self.mantissa = match self.mantissa.checked_mul(10) {
                        Some(v) => match v.checked_sub(digit) {
                            Some(v2) => v2,
                            None => return Err(ParseFixedError::Overflow),
                        },
                        None => return Err(ParseFixedError::Overflow),
                    };
                }

                if self.saw_dot {
                    self.frac_digits += 1;
                }
                self.saw_any_digit = true;
                Ok(self)
            }
            b'.' => {
                if self.saw_dot {
                    // Double dot
                    return Err(ParseFixedError::InvalidFormat);
                }
                self.saw_dot = true;
                Ok(self)
            }
            _ => Err(ParseFixedError::InvalidFormat),
        }
    }
}

impl<const D: u8> FixedI64<D> {
    /// Parse a decimal byte slice like `b"1.23"` or `b"-0.5"` into `FixedI64<D>`.
    ///
    /// Optimized single-pass parser for HFT hot paths:
    /// - **No i128**: accumulates directly into i64 (prices fit in 18 digits)
    /// - **Single scan**: no separate dot-find, digit-validate, or digit-parse passes
    /// - **POW10 lookup**: zero-padding via table lookup instead of multiply loop
    ///
    /// # Accepted formats
    ///
    /// - Integer-only: `b"123"`
    /// - Decimal: `b"1.23"`, `b".5"`, `b"0.5"`
    /// - Leading zeros: `b"007.50"`
    /// - Sign prefix: `b"-1.5"`, `b"+1.5"`
    ///
    /// # Rejected formats
    ///
    /// - Empty slices
    /// - Exponent notation (`b"1e5"`)
    /// - Double dots (`b"1..2"`)
    /// - Non-digit characters
    /// - Fractional digits exceeding D
    ///
    /// # Errors
    ///
    /// Returns [`ParseFixedError::InvalidFormat`] for malformed input,
    /// [`ParseFixedError::Overflow`] if the value exceeds `i64` range,
    /// or [`ParseFixedError::ExcessPrecision`] if fractional digits exceed D.
    pub const fn parse_decimal_bytes(bytes: &[u8]) -> Result<Self, ParseFixedError> {
        if bytes.is_empty() {
            return Err(ParseFixedError::InvalidFormat);
        }

        // Handle sign prefix
        let (negative, start) = match bytes[0] {
            b'-' => (true, 1),
            b'+' => (false, 1),
            _ => (false, 0),
        };

        if start >= bytes.len() {
            return Err(ParseFixedError::InvalidFormat);
        }

        // Single-pass accumulation
        let mut acc = Accumulator::new();
        let mut i = start;
        while i < bytes.len() {
            acc = match acc.feed(bytes[i]) {
                Ok(a) => a,
                Err(e) => return Err(e),
            };
            i += 1;
        }

        // Must have seen at least one digit ("." alone is invalid)
        if !acc.saw_any_digit {
            return Err(ParseFixedError::InvalidFormat);
        }

        // Check fractional precision
        if acc.frac_digits > D {
            return Err(ParseFixedError::ExcessPrecision);
        }

        // Rescale: mantissa currently has `frac_digits` implied decimals.
        // We need D decimals, so multiply by 10^(D - frac_digits).
        let pad = D - acc.frac_digits;
        if pad > 18 {
            return Err(ParseFixedError::Overflow);
        }

        // Mantissa is accumulated in negative space (always <= 0).
        // For positive numbers, negate. For negative, keep as-is.
        // This handles i64::MIN correctly since |-i64::MIN| > i64::MAX.
        let signed_mantissa = if negative {
            // Already negative — keep as-is
            acc.mantissa
        } else {
            // Negate to get positive value
            match acc.mantissa.checked_neg() {
                Some(v) => v,
                None => return Err(ParseFixedError::Overflow),
            }
        };

        if pad == 0 {
            return Ok(Self::from_raw(signed_mantissa));
        }

        let scale = POW10_I64[pad as usize];
        let Some(scaled) = signed_mantissa.checked_mul(scale) else {
            return Err(ParseFixedError::Overflow);
        };

        Ok(Self::from_raw(scaled))
    }

    /// Parse a decimal string like `"1.23"` or `"-0.5"` into `FixedI64<D>`.
    ///
    /// Delegates to [`parse_decimal_bytes`](Self::parse_decimal_bytes).
    ///
    /// # Errors
    ///
    /// Same errors as [`parse_decimal_bytes`](Self::parse_decimal_bytes).
    pub const fn from_str_decimal(s: &str) -> Result<Self, ParseFixedError> {
        Self::parse_decimal_bytes(s.as_bytes())
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests use expect for clarity")]
mod tests {
    extern crate alloc;
    use alloc::format;

    use crate::FixedI64;

    use super::ParseFixedError;

    type F2 = FixedI64<2>;
    type F6 = FixedI64<6>;
    type F8 = FixedI64<8>;

    #[test]
    fn parse_integer() {
        let v = F6::from_str_decimal("42").expect("valid");
        assert_eq!(v.to_raw(), 42_000_000);
    }

    #[test]
    fn parse_with_decimal() {
        let v = F6::from_str_decimal("1.5").expect("valid");
        assert_eq!(v.to_raw(), 1_500_000);
    }

    #[test]
    fn parse_full_precision() {
        let v = F6::from_str_decimal("1.234567").expect("valid");
        assert_eq!(v.to_raw(), 1_234_567);
    }

    #[test]
    fn parse_zero_variants() {
        assert_eq!(F6::from_str_decimal("0").expect("ok").to_raw(), 0);
        assert_eq!(F6::from_str_decimal("0.0").expect("ok").to_raw(), 0);
        assert_eq!(F6::from_str_decimal("0.000000").expect("ok").to_raw(), 0);
    }

    #[test]
    fn parse_negative_zero() {
        let v = F6::from_str_decimal("-0").expect("valid");
        assert_eq!(v.to_raw(), 0);
    }

    #[test]
    fn parse_negative_zero_decimal() {
        let v = F6::from_str_decimal("-0.0").expect("valid");
        assert_eq!(v.to_raw(), 0);
    }

    #[test]
    fn parse_leading_zeros() {
        let v = F6::from_str_decimal("007.500000").expect("valid");
        assert_eq!(v.to_raw(), 7_500_000);
    }

    #[test]
    fn parse_short_fraction_zero_pads() {
        // "1.5" at D=6 -> frac = 5 * 10^5 = 500_000
        let v = F6::from_str_decimal("1.5").expect("valid");
        assert_eq!(v.to_raw(), 1_500_000);
    }

    #[test]
    fn parse_no_whole_part() {
        let v = F6::from_str_decimal(".5").expect("valid");
        assert_eq!(v.to_raw(), 500_000);
    }

    #[test]
    fn parse_d2() {
        let v = F2::from_str_decimal("99.99").expect("valid");
        assert_eq!(v.to_raw(), 9999);
    }

    #[test]
    fn parse_negative() {
        let v = F6::from_str_decimal("-1.5").expect("valid");
        assert_eq!(v.to_raw(), -1_500_000);
    }

    #[test]
    fn parse_positive_sign() {
        let v = F6::from_str_decimal("+1.5").expect("valid");
        assert_eq!(v.to_raw(), 1_500_000);
    }

    // --- error cases ---

    #[test]
    fn parse_empty_string() {
        assert_eq!(
            F6::from_str_decimal(""),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_non_numeric() {
        assert_eq!(
            F6::from_str_decimal("abc"),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_exponent_rejected() {
        assert_eq!(
            F6::from_str_decimal("1e5"),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_double_dot() {
        assert_eq!(
            F6::from_str_decimal("1..2"),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_excess_precision() {
        assert_eq!(
            F2::from_str_decimal("1.123"),
            Err(ParseFixedError::ExcessPrecision)
        );
    }

    #[test]
    fn parse_overflow() {
        // A number way too large for i64
        assert_eq!(
            F6::from_str_decimal("99999999999999999999"),
            Err(ParseFixedError::Overflow)
        );
    }

    #[test]
    fn parse_just_dot() {
        assert_eq!(
            F6::from_str_decimal("."),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_just_sign() {
        assert_eq!(
            F6::from_str_decimal("-"),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    // --- roundtrip ---

    #[test]
    fn display_parse_roundtrip() {
        let values = [
            F6::ZERO,
            F6::ONE,
            F6::from_raw(1_500_000),
            F6::from_raw(-1_500_000),
            F6::from_raw(1),
            F6::from_raw(-1),
        ];
        for v in values {
            let s = format!("{v}");
            let parsed = F6::from_str_decimal(&s).expect("roundtrip should work");
            assert_eq!(v, parsed, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn display_parse_roundtrip_max() {
        let s = format!("{}", F6::MAX);
        let parsed = F6::from_str_decimal(&s).expect("MAX roundtrip");
        assert_eq!(F6::MAX, parsed);
    }

    #[test]
    fn display_parse_roundtrip_min() {
        let s = format!("{}", F6::MIN);
        let parsed = F6::from_str_decimal(&s).expect("MIN roundtrip");
        assert_eq!(F6::MIN, parsed);
    }

    #[test]
    fn parse_d8_precision() {
        let v = F8::from_str_decimal("0.00000001").expect("valid");
        assert_eq!(v.to_raw(), 1);
    }

    // --- parse_decimal_bytes tests ---

    #[test]
    fn parse_bytes_short_price() {
        let result = FixedI64::<6>::parse_decimal_bytes(b"0.53");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(530_000)));
    }

    #[test]
    fn parse_bytes_medium_price() {
        let result = FixedI64::<2>::parse_decimal_bytes(b"67396.70");
        assert_eq!(result, Ok(FixedI64::<2>::from_raw(6_739_670)));
    }

    #[test]
    fn parse_bytes_integer_only() {
        let result = FixedI64::<6>::parse_decimal_bytes(b"67396");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(67_396_000_000)));
    }

    #[test]
    fn parse_bytes_negative() {
        let result = FixedI64::<6>::parse_decimal_bytes(b"-1.5");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(-1_500_000)));
    }

    #[test]
    fn parse_bytes_no_whole_part() {
        let result = FixedI64::<6>::parse_decimal_bytes(b".5");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(500_000)));
    }

    #[test]
    fn parse_bytes_leading_zeros() {
        let result = FixedI64::<6>::parse_decimal_bytes(b"007.50");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(7_500_000)));
    }

    #[test]
    fn parse_bytes_trailing_zero_preserved() {
        let result = FixedI64::<2>::parse_decimal_bytes(b"0.10");
        assert_eq!(result, Ok(FixedI64::<2>::from_raw(10)));
    }

    #[test]
    fn parse_bytes_empty_fails() {
        assert_eq!(
            FixedI64::<6>::parse_decimal_bytes(b""),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_bytes_malformed_fails() {
        assert_eq!(
            FixedI64::<6>::parse_decimal_bytes(b"abc"),
            Err(ParseFixedError::InvalidFormat)
        );
    }

    #[test]
    fn parse_bytes_excess_precision_fails() {
        assert_eq!(
            FixedI64::<2>::parse_decimal_bytes(b"1.234"),
            Err(ParseFixedError::ExcessPrecision)
        );
    }

    #[test]
    fn parse_bytes_agrees_with_str() {
        let inputs = &["0.53", "67396.70", "-42.000001", "123456", ".5", "007.50"];
        for input in inputs {
            let from_str = FixedI64::<6>::from_str_decimal(input);
            let from_bytes = FixedI64::<6>::parse_decimal_bytes(input.as_bytes());
            assert_eq!(from_str, from_bytes, "mismatch for {input:?}");
        }
    }

    // --- HFT price format tests ---

    #[test]
    fn parse_binance_btc_price() {
        // Typical Binance BTC/USDT price
        let result = FixedI64::<2>::parse_decimal_bytes(b"72681.70");
        assert_eq!(result, Ok(FixedI64::<2>::from_raw(7_268_170)));
    }

    #[test]
    fn parse_binance_btc_qty() {
        // Typical Binance BTC quantity
        let result = FixedI64::<3>::parse_decimal_bytes(b"4.160");
        assert_eq!(result, Ok(FixedI64::<3>::from_raw(4160)));
    }

    #[test]
    fn parse_polymarket_price() {
        // Typical Polymarket contract price
        let result = FixedI64::<6>::parse_decimal_bytes(b"0.53");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(530_000)));
    }

    #[test]
    fn parse_polymarket_size() {
        // Typical Polymarket order size
        let result = FixedI64::<6>::parse_decimal_bytes(b"250.0");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(250_000_000)));
    }

    #[test]
    fn parse_max_18_digits_no_overflow() {
        // 18 digits is the max we support without overflow
        // 123456789012345678 fits in i64 (max ~9.2e18)
        let result = FixedI64::<0>::parse_decimal_bytes(b"123456789012345678");
        assert_eq!(result, Ok(FixedI64::<0>::from_raw(123_456_789_012_345_678)));
    }

    #[test]
    fn parse_19_digits_fits() {
        // 19 digits that fit in i64 (i64::MAX = 9223372036854775807)
        let result = FixedI64::<0>::parse_decimal_bytes(b"9223372036854775807");
        assert_eq!(result, Ok(FixedI64::<0>::from_raw(i64::MAX)));
    }

    #[test]
    fn parse_19_digits_overflows() {
        // 19 digits that exceed i64::MAX
        assert_eq!(
            FixedI64::<0>::parse_decimal_bytes(b"9223372036854775808"),
            Err(ParseFixedError::Overflow)
        );
    }

    #[test]
    fn parse_20_digits_overflows() {
        // 20 digits always overflows
        assert_eq!(
            FixedI64::<0>::parse_decimal_bytes(b"12345678901234567890"),
            Err(ParseFixedError::Overflow)
        );
    }

    #[test]
    fn parse_many_leading_zeros() {
        // 20 chars but value is 1 — leading zeros must not count toward overflow
        let result = FixedI64::<0>::parse_decimal_bytes(b"00000000000000000001");
        assert_eq!(result, Ok(FixedI64::<0>::from_raw(1)));
    }

    #[test]
    fn parse_leading_zeros_with_decimal() {
        let result = FixedI64::<6>::parse_decimal_bytes(b"000000.500000");
        assert_eq!(result, Ok(FixedI64::<6>::from_raw(500_000)));
    }
}
