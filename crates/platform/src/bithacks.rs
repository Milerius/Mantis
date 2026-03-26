//! Bit manipulation utilities for power-of-two and alignment operations.

/// Returns `true` if `n` is a power of two.
///
/// Zero is not considered a power of two.
#[must_use]
pub const fn is_power_of_two(n: usize) -> bool {
    n.is_power_of_two()
}

/// Returns the smallest power of two greater than or equal to `n`.
///
/// Delegates to [`usize::next_power_of_two`].
#[must_use]
pub const fn next_power_of_two(n: usize) -> usize {
    n.next_power_of_two()
}

/// Returns floor(log2(n)).
///
/// # Panics
///
/// Panics if `n == 0`.
#[must_use]
pub const fn log2_floor(n: usize) -> u32 {
    assert!(n > 0, "log2_floor: n must be greater than zero");
    (usize::BITS - 1) - n.leading_zeros()
}

/// Returns the number of trailing zero bits in `n`.
///
/// Returns `usize::BITS` when `n == 0`, consistent with the semantics of
/// [`usize::trailing_zeros`] extended to the all-zero case.
#[must_use]
pub const fn trailing_zeros(n: usize) -> u32 {
    n.trailing_zeros()
}

/// Rounds `value` up to the nearest multiple of `alignment`.
///
/// `alignment` must be a power of two.
///
/// # Panics
///
/// Panics if `alignment` is not a power of two.
#[must_use]
pub const fn round_up(value: usize, alignment: usize) -> usize {
    assert!(
        is_power_of_two(alignment),
        "round_up: alignment must be a power of two"
    );
    (value + alignment - 1) & !(alignment - 1)
}

/// Returns the ceiling of `a / b`.
///
/// This function is variable-time; do not use on secret inputs.
///
/// # Panics
///
/// Panics if `b == 0`.
#[must_use]
pub const fn ceil_div_vartime(a: usize, b: usize) -> usize {
    assert!(b > 0, "ceil_div_vartime: divisor must be greater than zero");
    a.div_ceil(b)
}

#[cfg(test)]
mod tests {
    use super::{
        ceil_div_vartime, is_power_of_two, log2_floor, next_power_of_two, round_up, trailing_zeros,
    };

    #[test]
    fn power_of_two_basics() {
        assert!(!is_power_of_two(0));
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(2));
        assert!(!is_power_of_two(3));
        assert!(is_power_of_two(4));
        assert!(is_power_of_two(1024));
        assert!(!is_power_of_two(1023));
    }

    #[test]
    fn next_power_of_two_basics() {
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(5), 8);
        assert_eq!(next_power_of_two(1024), 1024);
        assert_eq!(next_power_of_two(1025), 2048);
    }

    #[test]
    fn log2_floor_basics() {
        assert_eq!(log2_floor(1), 0);
        assert_eq!(log2_floor(2), 1);
        assert_eq!(log2_floor(3), 1);
        assert_eq!(log2_floor(4), 2);
        assert_eq!(log2_floor(7), 2);
        assert_eq!(log2_floor(8), 3);
        assert_eq!(log2_floor(1024), 10);
    }

    #[test]
    #[should_panic(expected = "log2_floor: n must be greater than zero")]
    fn log2_floor_zero_panics() {
        let _ = log2_floor(0);
    }

    #[test]
    fn log2_floor_usize_max() {
        assert_eq!(log2_floor(usize::MAX), usize::BITS - 1);
    }

    #[test]
    fn trailing_zeros_basics() {
        assert_eq!(trailing_zeros(0), usize::BITS);
        assert_eq!(trailing_zeros(1), 0);
        assert_eq!(trailing_zeros(2), 1);
        assert_eq!(trailing_zeros(4), 2);
        assert_eq!(trailing_zeros(6), 1);
        assert_eq!(trailing_zeros(1024), 10);
    }

    #[test]
    fn round_up_basics() {
        assert_eq!(round_up(0, 4), 0);
        assert_eq!(round_up(1, 4), 4);
        assert_eq!(round_up(4, 4), 4);
        assert_eq!(round_up(5, 4), 8);
        assert_eq!(round_up(0, 128), 0);
        assert_eq!(round_up(1, 128), 128);
        assert_eq!(round_up(128, 128), 128);
        assert_eq!(round_up(129, 128), 256);
    }

    #[test]
    #[should_panic(expected = "round_up: alignment must be a power of two")]
    fn round_up_non_power_panics() {
        let _ = round_up(1, 3);
    }

    #[test]
    fn ceil_div_basics() {
        assert_eq!(ceil_div_vartime(0, 1), 0);
        assert_eq!(ceil_div_vartime(1, 1), 1);
        assert_eq!(ceil_div_vartime(5, 2), 3);
        assert_eq!(ceil_div_vartime(10, 3), 4);
        assert_eq!(ceil_div_vartime(9, 3), 3);
    }

    #[test]
    #[should_panic(expected = "ceil_div_vartime: divisor must be greater than zero")]
    fn ceil_div_zero_panics() {
        let _ = ceil_div_vartime(1, 0);
    }
}
