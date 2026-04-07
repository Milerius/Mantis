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

/// Cache-line-sized padding using the native line size for the target.
/// 64 bytes on x86_64, 128 bytes on aarch64 (Apple Silicon).
/// Use this for hot-path structures where minimizing cache footprint matters.
#[derive(Debug)]
#[cfg_attr(target_arch = "x86_64", repr(align(64)))]
#[cfg_attr(not(target_arch = "x86_64"), repr(align(128)))]
pub struct CacheLine<T> {
    value: T,
}

impl<T> CacheLine<T> {
    /// Wrap a value with native cache-line alignment.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self { value }
    }
}

impl<T> Deref for CacheLine<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for CacheLine<T> {
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

    #[test]
    fn cache_line_native_alignment() {
        // On x86_64 this is 64, on aarch64 this is 128
        let align = core::mem::align_of::<CacheLine<u64>>();
        assert!(align == 64 || align == 128);
    }

    #[test]
    fn cache_line_deref() {
        let padded = CacheLine::new(42u64);
        assert_eq!(*padded, 42);
    }
}
