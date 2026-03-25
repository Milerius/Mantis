//! Struct layout and cache-line analysis for the Mantis SDK.
//!
//! Reports size, alignment, field offsets, and cache-line occupancy
//! for hot-path data structures.

#![deny(unsafe_code)]

use core::mem;

/// Layout information for a type.
#[derive(Debug, Clone)]
pub struct LayoutInfo {
    /// Type name.
    pub name: String,
    /// Size in bytes.
    pub size: usize,
    /// Alignment in bytes.
    pub align: usize,
    /// Number of cache lines occupied (assuming 64-byte lines).
    pub cache_lines: usize,
}

/// Inspect the layout of a type.
#[must_use]
pub fn inspect<T>(name: &str) -> LayoutInfo {
    let size = mem::size_of::<T>();
    let align = mem::align_of::<T>();
    let cache_lines = size.div_ceil(64);
    LayoutInfo {
        name: name.to_owned(),
        size,
        align,
        cache_lines,
    }
}

impl std::fmt::Display for LayoutInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Type: {}", self.name)?;
        writeln!(f, "  size:        {} bytes", self.size)?;
        writeln!(f, "  align:       {} bytes", self.align)?;
        writeln!(f, "  cache lines: {} (64B)", self.cache_lines)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_u64() {
        let info = inspect::<u64>("u64");
        assert_eq!(info.size, 8);
        assert_eq!(info.align, 8);
        assert_eq!(info.cache_lines, 1);
    }

    #[test]
    fn inspect_large_type() {
        let info = inspect::<[u8; 128]>("[u8; 128]");
        assert_eq!(info.size, 128);
        assert_eq!(info.cache_lines, 2);
    }
}
