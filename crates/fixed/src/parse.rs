//! Decimal string parsing for `FixedI64`.

use core::fmt;

use crate::FixedI64;

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

/// Returns `true` if every byte in `s` is an ASCII digit.
const fn all_digits(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        if !s[i].is_ascii_digit() {
            return false;
        }
        i += 1;
    }
    true
}

/// Parse an ASCII digit string as `i128`. Returns `None` on overflow or empty input.
/// Leading zeros are allowed.
const fn parse_digits(s: &[u8]) -> Option<i128> {
    if s.is_empty() {
        return None;
    }
    let mut acc: i128 = 0;
    let mut i = 0;
    while i < s.len() {
        let d = (s[i] - b'0') as i128;
        acc = match acc.checked_mul(10) {
            Some(v) => v,
            None => return None,
        };
        acc = match acc.checked_add(d) {
            Some(v) => v,
            None => return None,
        };
        i += 1;
    }
    Some(acc)
}

/// Find the index of the first `.` in `s`, or `None`.
const fn find_dot(s: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'.' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Check if byte slice contains a specific byte.
const fn contains_byte(s: &[u8], b: u8) -> bool {
    let mut i = 0;
    while i < s.len() {
        if s[i] == b {
            return true;
        }
        i += 1;
    }
    false
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "i128-to-i64 cast is guarded by range check"
)]
impl<const D: u8> FixedI64<D> {
    /// Parse a decimal string like `"1.23"` or `"-0.5"` into `FixedI64<D>`.
    ///
    /// Accepts:
    /// - Optional sign prefix (`-` or `+`)
    /// - Integer-only: `"123"`
    /// - Decimal: `"1.23"`, `".5"`, `"0.5"`
    /// - Leading zeros: `"007.50"`
    ///
    /// Rejects:
    /// - Empty strings
    /// - Exponent notation (`"1e5"`)
    /// - Double dots (`"1..2"`)
    /// - Non-digit characters
    /// - Fractional digits exceeding D
    ///
    /// # Errors
    ///
    /// Returns [`ParseFixedError::InvalidFormat`] for malformed input,
    /// [`ParseFixedError::Overflow`] if the value exceeds `i64` range,
    /// or [`ParseFixedError::ExcessPrecision`] if fractional digits exceed D.
    pub const fn from_str_decimal(s: &str) -> Result<Self, ParseFixedError> {
        let bytes = s.as_bytes();
        if bytes.is_empty() {
            return Err(ParseFixedError::InvalidFormat);
        }

        // Handle sign
        let (negative, rest) = match bytes[0] {
            b'-' => (true, bytes.split_at(1).1),
            b'+' => (false, bytes.split_at(1).1),
            _ => (false, bytes),
        };

        if rest.is_empty() {
            return Err(ParseFixedError::InvalidFormat);
        }

        // Reject exponent notation
        if contains_byte(rest, b'e') || contains_byte(rest, b'E') {
            return Err(ParseFixedError::InvalidFormat);
        }

        // Split on decimal point
        let (whole_bytes, frac_bytes) = match find_dot(rest) {
            None => {
                // No decimal point: integer only
                if !all_digits(rest) {
                    return Err(ParseFixedError::InvalidFormat);
                }
                (rest, &[] as &[u8])
            }
            Some(dot_idx) => {
                let (w, after_dot) = rest.split_at(dot_idx);
                // after_dot starts with '.', skip it
                let f = after_dot.split_at(1).1;

                // Reject double dots
                if contains_byte(f, b'.') {
                    return Err(ParseFixedError::InvalidFormat);
                }

                // Validate digits
                if !w.is_empty() && !all_digits(w) {
                    return Err(ParseFixedError::InvalidFormat);
                }
                if !f.is_empty() && !all_digits(f) {
                    return Err(ParseFixedError::InvalidFormat);
                }

                // ".5" is valid (empty whole part), but "." alone is not
                if w.is_empty() && f.is_empty() {
                    return Err(ParseFixedError::InvalidFormat);
                }

                (w, f)
            }
        };

        // Check excess precision
        if frac_bytes.len() > D as usize {
            return Err(ParseFixedError::ExcessPrecision);
        }

        // Parse whole part
        let whole: i128 = if whole_bytes.is_empty() {
            0
        } else {
            match parse_digits(whole_bytes) {
                Some(v) => v,
                None => return Err(ParseFixedError::Overflow),
            }
        };

        // Parse fractional part, zero-padding to D digits
        let frac: i128 = if frac_bytes.is_empty() {
            0
        } else {
            let Some(base) = parse_digits(frac_bytes) else {
                return Err(ParseFixedError::Overflow);
            };
            // Multiply by 10^(D - frac_len) to zero-pad
            let pad = D as usize - frac_bytes.len();
            let mut result = base;
            let mut p = 0;
            while p < pad {
                result = match result.checked_mul(10) {
                    Some(v) => v,
                    None => return Err(ParseFixedError::Overflow),
                };
                p += 1;
            }
            result
        };

        // Combine: whole * SCALE + frac
        let scale = Self::SCALE as i128;
        let combined = match whole.checked_mul(scale) {
            Some(v) => match v.checked_add(frac) {
                Some(v2) => v2,
                None => return Err(ParseFixedError::Overflow),
            },
            None => return Err(ParseFixedError::Overflow),
        };

        // Apply sign
        let signed = if negative { -combined } else { combined };

        // Narrow to i64
        if signed > (i64::MAX as i128) || signed < (i64::MIN as i128) {
            return Err(ParseFixedError::Overflow);
        }

        Ok(Self::from_raw(signed as i64))
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
}
