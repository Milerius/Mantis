//! Benchmark report metadata and serialization.

/// Benchmark report metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchReport {
    /// Name of the implementation being benchmarked.
    pub implementation: String,
    /// Target architecture.
    pub arch: String,
    /// Operating system.
    pub os: String,
    /// CPU model name.
    pub cpu: String,
    /// Rust compiler version.
    pub compiler: String,
    /// Enabled feature flags.
    pub features: Vec<String>,
    /// Workload results.
    pub results: Vec<WorkloadResult>,
}

/// Result for a single workload shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkloadResult {
    /// Workload name.
    pub workload: String,
    /// Element type.
    pub element_type: String,
    /// Operations per second.
    pub ops_per_sec: f64,
    /// Nanoseconds per operation.
    pub ns_per_op: f64,
    /// CPU cycles per operation (if available).
    pub cycles_per_op: Option<f64>,
    /// 50th percentile latency in nanoseconds.
    pub p50_ns: f64,
    /// 99th percentile latency in nanoseconds.
    pub p99_ns: f64,
    /// 99.9th percentile latency in nanoseconds.
    pub p999_ns: f64,
}

impl BenchReport {
    /// Create a new report with detected system info.
    #[must_use]
    pub fn detect(implementation: &str) -> Self {
        Self {
            implementation: implementation.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            os: std::env::consts::OS.to_owned(),
            cpu: detect_cpu_name(),
            compiler: detect_rustc_version(),
            features: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Export the report to JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

fn detect_cpu_name() -> String {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|n| n.trim().to_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "unknown".to_owned()
    }
}

fn detect_rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_fills_arch_and_os() {
        let report = BenchReport::detect("test");
        assert!(!report.arch.is_empty());
        assert!(!report.os.is_empty());
        assert!(!report.compiler.is_empty());
    }

    #[test]
    fn report_serializes_to_json() {
        let report = BenchReport {
            implementation: "SpscRing".to_owned(),
            arch: "x86_64".to_owned(),
            os: "linux".to_owned(),
            cpu: "AMD Ryzen 9 7950X".to_owned(),
            compiler: "rustc 1.85.0".to_owned(),
            features: vec!["asm".to_owned()],
            results: vec![WorkloadResult {
                workload: "single_item".to_owned(),
                element_type: "u64".to_owned(),
                ops_per_sec: 100_000_000.0,
                ns_per_op: 10.0,
                cycles_per_op: Some(35.0),
                p50_ns: 9.0,
                p99_ns: 15.0,
                p999_ns: 50.0,
            }],
        };
        let json = serde_json::to_string_pretty(&report).expect("serialization failed");
        assert!(json.contains("SpscRing"));
        assert!(json.contains("single_item"));
    }
}
