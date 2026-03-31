//! Constant-time arithmetic and comparison operations on `Ct<T>`.
//!
//! All operations use only bitwise/arithmetic primitives — no branches on
//! secret values. Maps from Constantine's `ct_routines.nim`.

use core::ops::{
    Add, AddAssign, BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Mul, MulAssign,
    Neg, Not, Shl, Shr, Sub, SubAssign,
};

use super::ct_types::{CTBool, Ct};

// ---------------------------------------------------------------------------
// Operator trait impls on Ct<T>
// ---------------------------------------------------------------------------

macro_rules! impl_ct_ops {
    ($($t:ty),+) => {
        $(
            // --- Bitwise ---

            impl BitAnd for Ct<$t> {
                type Output = Self;
                #[inline]
                fn bitand(self, rhs: Self) -> Self {
                    Ct(self.0 & rhs.0)
                }
            }

            impl BitAndAssign for Ct<$t> {
                #[inline]
                fn bitand_assign(&mut self, rhs: Self) {
                    self.0 &= rhs.0;
                }
            }

            impl BitOr for Ct<$t> {
                type Output = Self;
                #[inline]
                fn bitor(self, rhs: Self) -> Self {
                    Ct(self.0 | rhs.0)
                }
            }

            impl BitOrAssign for Ct<$t> {
                #[inline]
                fn bitor_assign(&mut self, rhs: Self) {
                    self.0 |= rhs.0;
                }
            }

            impl BitXor for Ct<$t> {
                type Output = Self;
                #[inline]
                fn bitxor(self, rhs: Self) -> Self {
                    Ct(self.0 ^ rhs.0)
                }
            }

            impl BitXorAssign for Ct<$t> {
                #[inline]
                fn bitxor_assign(&mut self, rhs: Self) {
                    self.0 ^= rhs.0;
                }
            }

            impl Not for Ct<$t> {
                type Output = Self;
                #[inline]
                fn not(self) -> Self {
                    Ct(!self.0)
                }
            }

            // --- Arithmetic ---

            impl Add for Ct<$t> {
                type Output = Self;
                #[inline]
                fn add(self, rhs: Self) -> Self {
                    Ct(self.0.wrapping_add(rhs.0))
                }
            }

            impl AddAssign for Ct<$t> {
                #[inline]
                fn add_assign(&mut self, rhs: Self) {
                    self.0 = self.0.wrapping_add(rhs.0);
                }
            }

            impl Sub for Ct<$t> {
                type Output = Self;
                #[inline]
                fn sub(self, rhs: Self) -> Self {
                    Ct(self.0.wrapping_sub(rhs.0))
                }
            }

            impl SubAssign for Ct<$t> {
                #[inline]
                fn sub_assign(&mut self, rhs: Self) {
                    self.0 = self.0.wrapping_sub(rhs.0);
                }
            }

            impl Mul for Ct<$t> {
                type Output = Self;
                #[inline]
                fn mul(self, rhs: Self) -> Self {
                    Ct(self.0.wrapping_mul(rhs.0))
                }
            }

            impl MulAssign for Ct<$t> {
                #[inline]
                fn mul_assign(&mut self, rhs: Self) {
                    self.0 = self.0.wrapping_mul(rhs.0);
                }
            }

            // Two's-complement negation: 0 - x.
            impl Neg for Ct<$t> {
                type Output = Self;
                #[inline]
                fn neg(self) -> Self {
                    Ct(self.0.wrapping_neg())
                }
            }

            // --- Shifts (rhs is u32 per Rust convention) ---

            impl Shr<u32> for Ct<$t> {
                type Output = Self;
                #[inline]
                fn shr(self, rhs: u32) -> Self {
                    Ct(self.0.wrapping_shr(rhs))
                }
            }

            impl Shl<u32> for Ct<$t> {
                type Output = Self;
                #[inline]
                fn shl(self, rhs: u32) -> Self {
                    Ct(self.0.wrapping_shl(rhs))
                }
            }

            // --- Comparison methods returning CTBool<T> ---

            impl Ct<$t> {
                /// Returns `CTBool` true when the MSB is set.
                ///
                /// Unsigned right shift fills with zeros, so shifting
                /// by BITS-1 yields 0 or 1.
                #[inline]
                #[must_use]
                pub fn is_msb_set(self) -> CTBool<$t> {
                    const MSB: u32 = <$t>::BITS - 1;
                    CTBool(self >> MSB)
                }

                /// Returns `CTBool` true when `self == other`.
                #[inline]
                #[must_use]
                pub fn ct_eq(self, other: Self) -> CTBool<$t> {
                    !self.ct_ne(other)
                }

                /// Returns `CTBool` true when `self != other`.
                #[inline]
                #[must_use]
                pub fn ct_ne(self, other: Self) -> CTBool<$t> {
                    // (z | -z) >> MSB: if z != 0, MSB of (z | -z) is set.
                    let z = self ^ other;
                    (z | (-z)).is_msb_set()
                }

                /// Returns `CTBool` true when `self < other`.
                ///
                /// Algorithm (constant-time): from Constantine's `<` template.
                /// `isMsbSet(x ^ ((x ^ y) | ((x - y) ^ y)))`
                #[inline]
                #[must_use]
                pub fn ct_lt(self, other: Self) -> CTBool<$t> {
                    let x = self;
                    let y = other;
                    (x ^ ((x ^ y) | ((x - y) ^ y))).is_msb_set()
                }

                /// Returns `CTBool` true when `self <= other`.
                #[inline]
                #[must_use]
                pub fn ct_le(self, other: Self) -> CTBool<$t> {
                    !other.ct_lt(self)
                }

                /// Returns `CTBool` true when `self == 0`.
                #[inline]
                #[must_use]
                pub fn is_zero(self) -> CTBool<$t> {
                    !self.is_non_zero()
                }

                /// Returns `CTBool` true when `self != 0`.
                #[inline]
                #[must_use]
                pub fn is_non_zero(self) -> CTBool<$t> {
                    (self | (-self)).is_msb_set()
                }

                /// Constant-time conditional negate.
                ///
                /// Returns `-self` when `ctl` is true, `self` when `ctl` is false.
                /// Formula: `(x ^ -T(ctl)) + T(ctl)`.
                #[inline]
                #[must_use]
                pub fn cneg(self, ctl: CTBool<$t>) -> Self {
                    let mask = -ctl.0;
                    (self ^ mask) + ctl.0
                }
            }

            // --- Boolean ops on CTBool<T> ---

            impl BitAnd for CTBool<$t> {
                type Output = Self;
                #[inline]
                fn bitand(self, rhs: Self) -> Self {
                    CTBool(self.0 & rhs.0)
                }
            }

            impl BitOr for CTBool<$t> {
                type Output = Self;
                #[inline]
                fn bitor(self, rhs: Self) -> Self {
                    CTBool(self.0 | rhs.0)
                }
            }

            impl BitXor for CTBool<$t> {
                type Output = Self;
                #[inline]
                fn bitxor(self, rhs: Self) -> Self {
                    // CTBool values are 0 or 1; XOR of 0/1 values equals != test.
                    CTBool(self.0 ^ rhs.0)
                }
            }

            impl Not for CTBool<$t> {
                type Output = Self;
                #[inline]
                fn not(self) -> Self {
                    // Flip bit 0 only: val ^ 1.
                    CTBool(self.0 ^ CTBool::<$t>::ctrue().0)
                }
            }
        )+
    };
}

impl_ct_ops!(u8, u16, u32, u64, usize);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Bitwise ops ---

    #[test]
    fn bitwise_and() {
        let a = Ct::<u64>::new(0b1100);
        let b = Ct::<u64>::new(0b1010);
        assert_eq!((a & b).inner(), 0b1000);
    }

    #[test]
    fn bitwise_or() {
        let a = Ct::<u64>::new(0b1100);
        let b = Ct::<u64>::new(0b1010);
        assert_eq!((a | b).inner(), 0b1110);
    }

    #[test]
    fn bitwise_xor() {
        let a = Ct::<u64>::new(0b1100);
        let b = Ct::<u64>::new(0b1010);
        assert_eq!((a ^ b).inner(), 0b0110);
    }

    #[test]
    fn bitwise_not() {
        let a = Ct::<u8>::new(0b0000_1111);
        assert_eq!((!a).inner(), 0b1111_0000);
    }

    // --- Assign variants ---

    #[test]
    fn assign_ops() {
        let mut a = Ct::<u32>::new(0b1111);
        a &= Ct::new(0b1010);
        assert_eq!(a.inner(), 0b1010);

        let mut b = Ct::<u32>::new(0b0000);
        b |= Ct::new(0b1010);
        assert_eq!(b.inner(), 0b1010);

        let mut c = Ct::<u32>::new(0b1111);
        c ^= Ct::new(0b1010);
        assert_eq!(c.inner(), 0b0101);
    }

    // --- Arithmetic ops ---

    #[test]
    fn add_wrapping() {
        let a = Ct::<u8>::new(250);
        let b = Ct::<u8>::new(10);
        assert_eq!((a + b).inner(), 4u8); // 260 mod 256
    }

    #[test]
    fn sub_wrapping() {
        let a = Ct::<u8>::new(5);
        let b = Ct::<u8>::new(10);
        assert_eq!((a - b).inner(), 251u8); // wrapping
    }

    #[test]
    fn mul_wrapping() {
        let a = Ct::<u8>::new(200);
        let b = Ct::<u8>::new(2);
        assert_eq!((a * b).inner(), 144u8); // 400 mod 256
    }

    #[test]
    fn neg_twos_complement() {
        let a = Ct::<u8>::new(1);
        assert_eq!((-a).inner(), 255u8);
        let zero = Ct::<u8>::new(0);
        assert_eq!((-zero).inner(), 0u8);
    }

    #[test]
    fn assign_arith() {
        let mut a = Ct::<u32>::new(10);
        a += Ct::new(5);
        assert_eq!(a.inner(), 15);
        a -= Ct::new(3);
        assert_eq!(a.inner(), 12);
        a *= Ct::new(4);
        assert_eq!(a.inner(), 48);
    }

    // --- Shifts ---

    #[test]
    fn shift_right() {
        let a = Ct::<u64>::new(0x80);
        assert_eq!((a >> 3).inner(), 0x10u64);
    }

    #[test]
    fn shift_left() {
        let a = Ct::<u64>::new(1);
        assert_eq!((a << 7).inner(), 128u64);
    }

    #[test]
    fn shift_wrapping_does_not_panic() {
        // wrapping_shr/shl mask the shift amount — no panic in debug
        let a = Ct::<u32>::new(1);
        let _ = a >> 33; // masked to 1
        let _ = a << 33; // masked to 1
    }

    // --- is_msb_set ---

    #[test]
    fn is_msb_set_u8() {
        assert_eq!(Ct::<u8>::new(0x80).is_msb_set().inner(), 1u8);
        assert_eq!(Ct::<u8>::new(0xFF).is_msb_set().inner(), 1u8);
        assert_eq!(Ct::<u8>::new(0x7F).is_msb_set().inner(), 0u8);
        assert_eq!(Ct::<u8>::new(0x00).is_msb_set().inner(), 0u8);
    }

    #[test]
    fn is_msb_set_u64() {
        assert_eq!(Ct::<u64>::new(1u64 << 63).is_msb_set().inner(), 1u64);
        assert_eq!(Ct::<u64>::new(u64::MAX).is_msb_set().inner(), 1u64);
        assert_eq!(Ct::<u64>::new(u64::MAX >> 1).is_msb_set().inner(), 0u64);
    }

    // --- ct_eq / ct_ne ---

    #[test]
    fn ct_eq_equal() {
        assert_eq!(Ct::<u64>::new(42).ct_eq(Ct::new(42)).inner(), 1u64);
    }

    #[test]
    fn ct_eq_unequal() {
        assert_eq!(Ct::<u64>::new(42).ct_eq(Ct::new(43)).inner(), 0u64);
    }

    #[test]
    fn ct_ne() {
        assert_eq!(Ct::<u64>::new(42).ct_ne(Ct::new(43)).inner(), 1u64);
        assert_eq!(Ct::<u64>::new(42).ct_ne(Ct::new(42)).inner(), 0u64);
    }

    // Exhaustive check over u8 pairs
    #[test]
    fn ct_eq_exhaustive_u8() {
        for x in 0u8..=255 {
            for y in 0u8..=255 {
                let expected = u8::from(x == y);
                let got = Ct::<u8>::new(x).ct_eq(Ct::new(y)).inner();
                assert_eq!(got, expected, "ct_eq({x}, {y}) failed");
            }
        }
    }

    // --- ct_lt / ct_le ---

    #[test]
    fn ct_lt_basic() {
        assert_eq!(Ct::<u64>::new(3).ct_lt(Ct::new(5)).inner(), 1u64);
        assert_eq!(Ct::<u64>::new(5).ct_lt(Ct::new(3)).inner(), 0u64);
        assert_eq!(Ct::<u64>::new(5).ct_lt(Ct::new(5)).inner(), 0u64);
    }

    #[test]
    fn ct_le_basic() {
        assert_eq!(Ct::<u64>::new(3).ct_le(Ct::new(5)).inner(), 1u64);
        assert_eq!(Ct::<u64>::new(5).ct_le(Ct::new(3)).inner(), 0u64);
        assert_eq!(Ct::<u64>::new(5).ct_le(Ct::new(5)).inner(), 1u64);
    }

    // Exhaustive check over u8 pairs
    #[test]
    fn ct_lt_exhaustive_u8() {
        for x in 0u8..=255 {
            for y in 0u8..=255 {
                let expected_lt = u8::from(x < y);
                let expected_le = u8::from(x <= y);
                let got_lt = Ct::<u8>::new(x).ct_lt(Ct::new(y)).inner();
                let got_le = Ct::<u8>::new(x).ct_le(Ct::new(y)).inner();
                assert_eq!(got_lt, expected_lt, "ct_lt({x}, {y}) failed");
                assert_eq!(got_le, expected_le, "ct_le({x}, {y}) failed");
            }
        }
    }

    // --- is_zero / is_non_zero ---

    #[test]
    fn is_zero() {
        assert_eq!(Ct::<u64>::new(0).is_zero().inner(), 1u64);
        assert_eq!(Ct::<u64>::new(1).is_zero().inner(), 0u64);
        assert_eq!(Ct::<u64>::new(u64::MAX).is_zero().inner(), 0u64);
    }

    #[test]
    fn is_non_zero() {
        assert_eq!(Ct::<u64>::new(0).is_non_zero().inner(), 0u64);
        assert_eq!(Ct::<u64>::new(1).is_non_zero().inner(), 1u64);
        assert_eq!(Ct::<u64>::new(u64::MAX).is_non_zero().inner(), 1u64);
    }

    // --- cneg ---

    #[test]
    fn cneg_true_negates() {
        let x = Ct::<u8>::new(3);
        let result = x.cneg(CTBool::<u8>::ctrue());
        assert_eq!(result.inner(), (0u8).wrapping_sub(3));
    }

    #[test]
    fn cneg_false_identity() {
        let x = Ct::<u8>::new(3);
        let result = x.cneg(CTBool::<u8>::cfalse());
        assert_eq!(result.inner(), 3u8);
    }

    #[test]
    fn cneg_exhaustive_u8() {
        for x in 0u8..=255 {
            let ct_true_result = Ct::<u8>::new(x).cneg(CTBool::<u8>::ctrue()).inner();
            let ct_false_result = Ct::<u8>::new(x).cneg(CTBool::<u8>::cfalse()).inner();
            assert_eq!(ct_true_result, x.wrapping_neg(), "cneg true failed for {x}");
            assert_eq!(ct_false_result, x, "cneg false failed for {x}");
        }
    }

    // --- CTBool boolean ops ---

    #[test]
    fn ctbool_not() {
        assert_eq!((!CTBool::<u64>::ctrue()).inner(), 0u64);
        assert_eq!((!CTBool::<u64>::cfalse()).inner(), 1u64);
    }

    #[test]
    fn ctbool_and() {
        let t = CTBool::<u64>::ctrue();
        let f = CTBool::<u64>::cfalse();
        assert_eq!((t & t).inner(), 1u64);
        assert_eq!((t & f).inner(), 0u64);
        assert_eq!((f & t).inner(), 0u64);
        assert_eq!((f & f).inner(), 0u64);
    }

    #[test]
    fn ctbool_or() {
        let t = CTBool::<u64>::ctrue();
        let f = CTBool::<u64>::cfalse();
        assert_eq!((t | t).inner(), 1u64);
        assert_eq!((t | f).inner(), 1u64);
        assert_eq!((f | t).inner(), 1u64);
        assert_eq!((f | f).inner(), 0u64);
    }

    #[test]
    fn ctbool_xor() {
        let t = CTBool::<u64>::ctrue();
        let f = CTBool::<u64>::cfalse();
        assert_eq!((t ^ t).inner(), 0u64);
        assert_eq!((t ^ f).inner(), 1u64);
        assert_eq!((f ^ t).inner(), 1u64);
        assert_eq!((f ^ f).inner(), 0u64);
    }

    // --- Smaller types sanity check ---

    #[test]
    fn u8_comparisons() {
        assert_eq!(Ct::<u8>::new(10).ct_eq(Ct::new(10)).inner(), 1u8);
        assert_eq!(Ct::<u8>::new(10).ct_lt(Ct::new(20)).inner(), 1u8);
        assert_eq!(Ct::<u8>::new(20).ct_lt(Ct::new(10)).inner(), 0u8);
    }

    #[test]
    fn u16_comparisons() {
        assert_eq!(Ct::<u16>::new(1000).ct_eq(Ct::new(1000)).inner(), 1u16);
        assert_eq!(Ct::<u16>::new(500).ct_lt(Ct::new(1000)).inner(), 1u16);
    }

    #[test]
    fn u32_comparisons() {
        assert_eq!(Ct::<u32>::new(0).is_zero().inner(), 1u32);
        assert_eq!(Ct::<u32>::new(u32::MAX).is_non_zero().inner(), 1u32);
    }

    #[test]
    fn usize_ops() {
        let a = Ct::<usize>::new(100);
        let b = Ct::<usize>::new(50);
        assert_eq!((a + b).inner(), 150usize);
        assert_eq!(a.ct_lt(b).inner(), 0usize);
        assert_eq!(b.ct_lt(a).inner(), 1usize);
    }
}
