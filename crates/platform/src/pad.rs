//! Cache-line padding to prevent false sharing.

use core::ops::{Deref, DerefMut};

/// Aligns the inner value to 128 bytes, covering both Intel (64B)
/// and Apple Silicon (128B) cache lines. Prevents false sharing
/// between adjacent atomics in the ring engine.
#[derive(Debug)]
#[repr(align(128))]
pub struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    /// Wrap a value with cache-line padding.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for CachePadded<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_padded_alignment() {
        assert_eq!(core::mem::align_of::<CachePadded<u64>>(), 128);
    }

    #[test]
    fn cache_padded_deref() {
        let padded = CachePadded::new(42u64);
        assert_eq!(*padded, 42);
    }

    #[test]
    fn cache_padded_deref_mut() {
        let mut padded = CachePadded::new(42u64);
        *padded = 99;
        assert_eq!(*padded, 99);
    }
}
