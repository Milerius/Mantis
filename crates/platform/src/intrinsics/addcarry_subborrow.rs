//! Add-with-carry and subtract-with-borrow for multi-limb arithmetic.
//!
//! Portable baseline using widening arithmetic (u32 → u64, u64 → u128).
//! Maps to Constantine's `addcarry_subborrow.nim`.

use crate::constant_time::{Borrow, Carry, Ct};

/// Addition with carry.
pub trait AddCarryOp: Sized {
    /// Computes `(sum, carry_out) <- self + rhs + carry_in`.
    fn add_c(self, rhs: Self, carry_in: Carry) -> (Self, Carry);
}

/// Subtraction with borrow.
pub trait SubBorrowOp: Sized {
    /// Computes `(diff, borrow_out) <- self - rhs - borrow_in`.
    fn sub_b(self, rhs: Self, borrow_in: Borrow) -> (Self, Borrow);
}

impl AddCarryOp for Ct<u32> {
    #[inline]
    fn add_c(self, rhs: Self, carry_in: Carry) -> (Self, Carry) {
        let wide =
            u64::from(self.inner()) + u64::from(rhs.inner()) + u64::from(carry_in.inner());
        // Intentional truncation: low 32 bits are the sum, high 32 bits hold carry (0 or 1).
        #[expect(clippy::cast_possible_truncation, reason = "intentional widening-split pattern")]
        let sum = Ct::new(wide as u32);
        #[expect(clippy::cast_possible_truncation, reason = "intentional widening-split pattern")]
        let carry_out = Ct::new((wide >> 32) as u8);
        (sum, carry_out)
    }
}

impl SubBorrowOp for Ct<u32> {
    #[inline]
    fn sub_b(self, rhs: Self, borrow_in: Borrow) -> (Self, Borrow) {
        let wide =
            u64::from(self.inner())
                .wrapping_sub(u64::from(rhs.inner()))
                .wrapping_sub(u64::from(borrow_in.inner()));
        // Intentional truncation: low 32 bits are the difference; high word is all-ones on borrow.
        #[expect(clippy::cast_possible_truncation, reason = "intentional widening-split pattern")]
        let diff = Ct::new(wide as u32);
        // Mask to extract the single borrow bit from the all-ones high word.
        // & 1 ensures the value is 0 or 1, which fits in u8 without truncation.
        let borrow_out = Ct::new(((wide >> 32) & 1) as u8);
        (diff, borrow_out)
    }
}

impl AddCarryOp for Ct<u64> {
    #[inline]
    fn add_c(self, rhs: Self, carry_in: Carry) -> (Self, Carry) {
        let wide =
            u128::from(self.inner()) + u128::from(rhs.inner()) + u128::from(carry_in.inner());
        // Intentional truncation: low 64 bits are the sum, high 64 bits hold carry (0 or 1).
        #[expect(clippy::cast_possible_truncation, reason = "intentional widening-split pattern")]
        let sum = Ct::new(wide as u64);
        #[expect(clippy::cast_possible_truncation, reason = "intentional widening-split pattern")]
        let carry_out = Ct::new((wide >> 64) as u8);
        (sum, carry_out)
    }
}

impl SubBorrowOp for Ct<u64> {
    #[inline]
    fn sub_b(self, rhs: Self, borrow_in: Borrow) -> (Self, Borrow) {
        let wide =
            u128::from(self.inner())
                .wrapping_sub(u128::from(rhs.inner()))
                .wrapping_sub(u128::from(borrow_in.inner()));
        // Intentional truncation: low 64 bits are the difference; high word is all-ones on borrow.
        #[expect(clippy::cast_possible_truncation, reason = "intentional widening-split pattern")]
        let diff = Ct::new(wide as u64);
        // Mask to extract the single borrow bit from the all-ones high word.
        // & 1 ensures the value is 0 or 1, which fits in u8 without truncation.
        let borrow_out = Ct::new(((wide >> 64) & 1) as u8);
        (diff, borrow_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Ct<u32> AddCarryOp ---

    #[test]
    fn add_c_u32_no_carry() {
        let (sum, cout) = Ct::new(3u32).add_c(Ct::new(4u32), Ct::new(0u8));
        assert_eq!(sum.inner(), 7u32);
        assert_eq!(cout.inner(), 0u8);
    }

    #[test]
    fn add_c_u32_carry_in() {
        let (sum, cout) = Ct::new(3u32).add_c(Ct::new(4u32), Ct::new(1u8));
        assert_eq!(sum.inner(), 8u32);
        assert_eq!(cout.inner(), 0u8);
    }

    #[test]
    fn add_c_u32_carry_out() {
        let (sum, cout) = Ct::new(u32::MAX).add_c(Ct::new(1u32), Ct::new(0u8));
        assert_eq!(sum.inner(), 0u32);
        assert_eq!(cout.inner(), 1u8);
    }

    #[test]
    fn add_c_u32_both_carries() {
        let (sum, cout) = Ct::new(u32::MAX).add_c(Ct::new(1u32), Ct::new(1u8));
        assert_eq!(sum.inner(), 1u32);
        assert_eq!(cout.inner(), 1u8);
    }

    #[test]
    fn add_c_u32_chain_4_limbs() {
        // Simulate 256-bit add: [u32::MAX; 4] + [1, 0, 0, 0]
        let a = [Ct::new(u32::MAX); 4];
        let b = [Ct::new(1u32), Ct::new(0u32), Ct::new(0u32), Ct::new(0u32)];
        let mut carry = Ct::new(0u8);
        let mut result = [0u32; 4];
        for i in 0..4 {
            let (s, c) = a[i].add_c(b[i], carry);
            result[i] = s.inner();
            carry = c;
        }
        assert_eq!(result, [0u32, 0u32, 0u32, 0u32]);
        assert_eq!(carry.inner(), 1u8);
    }

    // --- Ct<u32> SubBorrowOp ---

    #[test]
    fn sub_b_u32_no_borrow() {
        let (diff, bout) = Ct::new(7u32).sub_b(Ct::new(3u32), Ct::new(0u8));
        assert_eq!(diff.inner(), 4u32);
        assert_eq!(bout.inner(), 0u8);
    }

    #[test]
    fn sub_b_u32_borrow_in() {
        let (diff, bout) = Ct::new(7u32).sub_b(Ct::new(3u32), Ct::new(1u8));
        assert_eq!(diff.inner(), 3u32);
        assert_eq!(bout.inner(), 0u8);
    }

    #[test]
    fn sub_b_u32_borrow_out() {
        let (diff, bout) = Ct::new(0u32).sub_b(Ct::new(1u32), Ct::new(0u8));
        assert_eq!(diff.inner(), u32::MAX);
        assert_eq!(bout.inner(), 1u8);
    }

    #[test]
    fn sub_b_u32_both_borrows() {
        let (diff, bout) = Ct::new(0u32).sub_b(Ct::new(1u32), Ct::new(1u8));
        assert_eq!(diff.inner(), u32::MAX - 1);
        assert_eq!(bout.inner(), 1u8);
    }

    #[test]
    fn sub_b_u32_chain_4_limbs() {
        // Simulate 256-bit sub: [0u32; 4] - [1, 0, 0, 0]
        let a = [Ct::new(0u32); 4];
        let b = [Ct::new(1u32), Ct::new(0u32), Ct::new(0u32), Ct::new(0u32)];
        let mut borrow = Ct::new(0u8);
        let mut result = [0u32; 4];
        for i in 0..4 {
            let (d, bo) = a[i].sub_b(b[i], borrow);
            result[i] = d.inner();
            borrow = bo;
        }
        assert_eq!(result, [u32::MAX, u32::MAX, u32::MAX, u32::MAX]);
        assert_eq!(borrow.inner(), 1u8);
    }

    // --- Ct<u64> AddCarryOp ---

    #[test]
    fn add_c_u64_no_carry() {
        let (sum, cout) = Ct::new(3u64).add_c(Ct::new(4u64), Ct::new(0u8));
        assert_eq!(sum.inner(), 7u64);
        assert_eq!(cout.inner(), 0u8);
    }

    #[test]
    fn add_c_u64_carry_in() {
        let (sum, cout) = Ct::new(3u64).add_c(Ct::new(4u64), Ct::new(1u8));
        assert_eq!(sum.inner(), 8u64);
        assert_eq!(cout.inner(), 0u8);
    }

    #[test]
    fn add_c_u64_carry_out() {
        let (sum, cout) = Ct::new(u64::MAX).add_c(Ct::new(1u64), Ct::new(0u8));
        assert_eq!(sum.inner(), 0u64);
        assert_eq!(cout.inner(), 1u8);
    }

    #[test]
    fn add_c_u64_both_carries() {
        let (sum, cout) = Ct::new(u64::MAX).add_c(Ct::new(1u64), Ct::new(1u8));
        assert_eq!(sum.inner(), 1u64);
        assert_eq!(cout.inner(), 1u8);
    }

    #[test]
    fn add_c_u64_chain_4_limbs() {
        // Simulate 256-bit add: [u64::MAX; 4] + [1, 0, 0, 0]
        let a = [Ct::new(u64::MAX); 4];
        let b = [Ct::new(1u64), Ct::new(0u64), Ct::new(0u64), Ct::new(0u64)];
        let mut carry = Ct::new(0u8);
        let mut result = [0u64; 4];
        for i in 0..4 {
            let (s, c) = a[i].add_c(b[i], carry);
            result[i] = s.inner();
            carry = c;
        }
        assert_eq!(result, [0u64, 0u64, 0u64, 0u64]);
        assert_eq!(carry.inner(), 1u8);
    }

    // --- Ct<u64> SubBorrowOp ---

    #[test]
    fn sub_b_u64_no_borrow() {
        let (diff, bout) = Ct::new(7u64).sub_b(Ct::new(3u64), Ct::new(0u8));
        assert_eq!(diff.inner(), 4u64);
        assert_eq!(bout.inner(), 0u8);
    }

    #[test]
    fn sub_b_u64_borrow_in() {
        let (diff, bout) = Ct::new(7u64).sub_b(Ct::new(3u64), Ct::new(1u8));
        assert_eq!(diff.inner(), 3u64);
        assert_eq!(bout.inner(), 0u8);
    }

    #[test]
    fn sub_b_u64_borrow_out() {
        let (diff, bout) = Ct::new(0u64).sub_b(Ct::new(1u64), Ct::new(0u8));
        assert_eq!(diff.inner(), u64::MAX);
        assert_eq!(bout.inner(), 1u8);
    }

    #[test]
    fn sub_b_u64_both_borrows() {
        let (diff, bout) = Ct::new(0u64).sub_b(Ct::new(1u64), Ct::new(1u8));
        assert_eq!(diff.inner(), u64::MAX - 1);
        assert_eq!(bout.inner(), 1u8);
    }

    #[test]
    fn sub_b_u64_chain_4_limbs() {
        // Simulate 256-bit sub: [0u64; 4] - [1, 0, 0, 0]
        let a = [Ct::new(0u64); 4];
        let b = [Ct::new(1u64), Ct::new(0u64), Ct::new(0u64), Ct::new(0u64)];
        let mut borrow = Ct::new(0u8);
        let mut result = [0u64; 4];
        for i in 0..4 {
            let (d, bo) = a[i].sub_b(b[i], borrow);
            result[i] = d.inner();
            borrow = bo;
        }
        assert_eq!(result, [u64::MAX, u64::MAX, u64::MAX, u64::MAX]);
        assert_eq!(borrow.inner(), 1u8);
    }
}
