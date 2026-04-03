//! Numeric constants and helpers for the Mantis SDK.

/// Power-of-10 lookup table for i64 domain.
/// `POW10_I64[d] == 10^d` for `d` in `0..=18`.
pub const POW10_I64: [i64; 19] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
    1_000_000_000_000_000_000,
];

/// Const accessor for power-of-10.
///
/// # Panics
///
/// Panics at compile time if `d > 18`.
#[must_use]
pub const fn pow10_i64(d: u8) -> i64 {
    POW10_I64[d as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow10_values_are_correct() {
        let mut expected: i64 = 1;
        for (i, &val) in POW10_I64.iter().enumerate() {
            assert_eq!(val, expected, "POW10_I64[{i}] wrong");
            if i < 18 {
                expected = expected.checked_mul(10).expect("overflow building expected");
            }
        }
    }

    #[test]
    fn pow10_accessor_matches_table() {
        for d in 0..=18u8 {
            assert_eq!(pow10_i64(d), POW10_I64[d as usize]);
        }
    }
}
