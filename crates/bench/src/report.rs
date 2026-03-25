//! Benchmark report metadata and serialization.

/// Benchmark report metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchReport {
    /// Name of the implementation being benchmarked.
    pub implementation: String,
    /// Target architecture (e.g., `"x86_64"`, `"aarch64"`).
    pub arch: String,
    /// Operating system.
    pub os: String,
    /// CPU model name.
    pub cpu: String,
}

impl BenchReport {
    /// Create a new report with detected system info.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            implementation: String::new(),
            arch: std::env::consts::ARCH.to_owned(),
            os: std::env::consts::OS.to_owned(),
            cpu: String::from("unknown"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_fills_arch_and_os() {
        let report = BenchReport::detect();
        assert!(!report.arch.is_empty());
        assert!(!report.os.is_empty());
    }
}
