//! Benchmark report metadata and serialization.

/// Benchmark report metadata.
///
/// Follows the report schema from Section 24-25 of the benchmark
/// philosophy doc: implementation, compiler, CPU, payload, capacity,
/// threading model, metrics, comparison against baseline.
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
    /// Compiler optimization flags (e.g., `"-C opt-level=3"`).
    pub compiler_flags: String,
    /// Threading model (e.g., `"spsc"`, `"mpsc"`).
    pub threading_model: String,
    /// Workload results.
    pub results: Vec<WorkloadResult>,
}

/// Result for a single workload shape.
///
/// Core metrics (throughput, latencies) are always populated.
/// Hardware counter metrics (instructions, cache, branches) are
/// optional — they require platform-specific `perf_event_open`
/// or equivalent and may not be available on all systems.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkloadResult {
    /// Workload name (e.g., `"single_item"`, `"burst_100"`).
    pub workload: String,
    /// Element type (e.g., `"u64"`, `"[u8; 64]"`).
    pub element_type: String,
    /// Ring capacity used for this workload.
    pub capacity: usize,

    // -- Throughput --
    /// Operations per second.
    pub ops_per_sec: f64,
    /// Nanoseconds per operation.
    pub ns_per_op: f64,

    // -- Latency percentiles --
    /// 50th percentile latency in nanoseconds.
    pub p50_ns: f64,
    /// 99th percentile latency in nanoseconds.
    pub p99_ns: f64,
    /// 99.9th percentile latency in nanoseconds.
    pub p999_ns: f64,

    // -- Hardware counters (optional, platform-dependent) --
    /// CPU cycles per operation.
    pub cycles_per_op: Option<f64>,
    /// Instructions per operation.
    pub instructions_per_op: Option<f64>,
    /// Branch misses per operation.
    pub branch_misses_per_op: Option<f64>,
    /// L1 cache misses per operation.
    pub l1_misses_per_op: Option<f64>,
    /// Last-level cache misses per operation.
    pub llc_misses_per_op: Option<f64>,

    // -- Queue health (from instrumented preset) --
    /// Push-full hit rate (0.0-1.0).
    pub full_rate: Option<f64>,
    /// Pop-empty hit rate (0.0-1.0).
    pub empty_rate: Option<f64>,
    /// Mean queue occupancy (0.0-1.0).
    pub mean_occupancy: Option<f64>,
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
            compiler_flags: String::new(),
            threading_model: String::new(),
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
            compiler_flags: "-C opt-level=3 -C target-cpu=native".to_owned(),
            threading_model: "spsc".to_owned(),
            results: vec![WorkloadResult {
                workload: "single_item".to_owned(),
                element_type: "u64".to_owned(),
                capacity: 1024,
                ops_per_sec: 100_000_000.0,
                ns_per_op: 10.0,
                p50_ns: 9.0,
                p99_ns: 15.0,
                p999_ns: 50.0,
                cycles_per_op: Some(35.0),
                instructions_per_op: Some(12.0),
                branch_misses_per_op: Some(0.3),
                l1_misses_per_op: Some(0.4),
                llc_misses_per_op: Some(0.01),
                full_rate: None,
                empty_rate: None,
                mean_occupancy: None,
            }],
        };
        let json = serde_json::to_string_pretty(&report).unwrap_or_default();
        assert!(json.contains("SpscRing"));
        assert!(json.contains("single_item"));
    }
}
