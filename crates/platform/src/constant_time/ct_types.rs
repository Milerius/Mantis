//! Fundamental constant-time types.
//!
//! `Ct<T>` wraps unsigned integers to prevent the compiler from optimizing
//! bitwise operations into conditional branches, which would leak timing
//! information about secret values.

use core::fmt;

/// Constant-time unsigned integer wrapper.
///
/// Prevents the compiler from optimizing bitwise operations on the inner value
/// into conditional branches that would leak timing information.
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
pub struct Ct<T>(pub(crate) T);

/// Constant-time boolean, restricted to values 0 or 1.
///
/// The inner `Ct<T>` must always hold either `0` (false) or `1` (true).
/// This invariant is upheld by all constructors and conversions.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CTBool<T>(pub(crate) Ct<T>);

/// Carry flag for addition chains.
pub type Carry = Ct<u8>;

/// Borrow flag for subtraction chains.
pub type Borrow = Ct<u8>;

/// Marker type for variable-time operations (effect tracking).
///
/// Use this as a parameter or return type to make the variable-time nature
/// of an operation explicit in the type system.
pub struct VarTime;

impl<T> Ct<T> {
    /// Wraps a value in a constant-time container.
    #[inline]
    pub const fn new(val: T) -> Self {
        Self(val)
    }

    /// Unwraps the inner value.
    ///
    /// Use sparingly — only at the boundary between constant-time and
    /// variable-time code.
    #[inline]
    pub fn inner(self) -> T
    where
        T: Copy,
    {
        self.0
    }
}

impl<T> fmt::Debug for Ct<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Ct(***)")
    }
}

impl<T> fmt::Debug for CTBool<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CTBool(***)")
    }
}

macro_rules! impl_ctbool {
    ($($t:ty),+) => {
        $(
            impl CTBool<$t> {
                /// Returns the constant-time representation of `true` (value 1).
                #[inline]
                pub const fn ctrue() -> Self {
                    Self(Ct(1))
                }

                /// Returns the constant-time representation of `false` (value 0).
                #[inline]
                pub const fn cfalse() -> Self {
                    Self(Ct(0))
                }

                /// Unwraps the inner unsigned value (0 or 1).
                ///
                /// Use sparingly — only at the boundary between constant-time
                /// and variable-time code.
                #[inline]
                pub fn inner(self) -> $t {
                    self.0.inner()
                }
            }

            impl From<bool> for CTBool<$t> {
                #[inline]
                fn from(b: bool) -> Self {
                    Self(Ct(b as $t))
                }
            }
        )+
    };
}

impl_ctbool!(u8, u16, u32, u64, usize);

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn ct_construction() {
        let ct = Ct::new(42u64);
        assert_eq!(ct.inner(), 42u64);
    }

    #[test]
    fn carry_borrow_are_ct_u8() {
        let c: Carry = Ct::new(1u8);
        let b: Borrow = Ct::new(0u8);
        assert_eq!(c.inner(), 1u8);
        assert_eq!(b.inner(), 0u8);
    }

    #[test]
    fn vartime_is_zst() {
        assert_eq!(size_of::<VarTime>(), 0);
    }

    #[test]
    #[cfg(feature = "std")]
    fn ct_debug_does_not_leak() {
        let ct = Ct::new(42u64);
        let s = std::format!("{ct:?}");
        assert!(!s.contains("42"), "debug output must not contain the value");
        assert_eq!(s, "Ct(***)");
    }

    #[test]
    fn ctbool_ctrue_cfalse() {
        assert_eq!(CTBool::<u64>::ctrue().inner(), 1u64);
        assert_eq!(CTBool::<u64>::cfalse().inner(), 0u64);
    }

    #[test]
    fn ctbool_from_bool() {
        assert_eq!(CTBool::<u64>::from(true).inner(), 1u64);
        assert_eq!(CTBool::<u64>::from(false).inner(), 0u64);
    }
}
