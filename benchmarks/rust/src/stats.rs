use serde::Serialize;

// ---------------------------------------------------------------------------
// CycleHistogram — fixed-size, allocation-free latency recorder
// ---------------------------------------------------------------------------

/// Number of direct-mapped 1-cycle-resolution buckets.
const DIRECT_BUCKETS: usize = 4096;

/// Number of logarithmic overflow buckets (covers 4096 .. ~4 billion cycles).
const LOG_BUCKETS: usize = 20;

/// A cycle-accurate histogram with 4096 direct-mapped buckets (1-cycle
/// resolution for values 0..4095) plus 20 log-bucketed overflow buckets.
pub struct CycleHistogram {
    direct: Box<[u64; DIRECT_BUCKETS]>,
    overflow: [u64; LOG_BUCKETS],
    count: u64,
    sum: u64,
    min: u64,
    max: u64,
}

impl CycleHistogram {
    pub fn new() -> Self {
        Self {
            direct: Box::new([0u64; DIRECT_BUCKETS]),
            overflow: [0u64; LOG_BUCKETS],
            count: 0,
            sum: 0,
            min: u64::MAX,
            max: 0,
        }
    }

    /// Record a single cycle measurement.
    #[inline]
    pub fn record(&mut self, cycles: u64) {
        self.count += 1;
        self.sum += cycles;
        if cycles < self.min {
            self.min = cycles;
        }
        if cycles > self.max {
            self.max = cycles;
        }

        if (cycles as usize) < DIRECT_BUCKETS {
            self.direct[cycles as usize] += 1;
        } else {
            let bucket = log_bucket(cycles);
            self.overflow[bucket] += 1;
        }
    }

    /// Walk the histogram to find the real percentile value (not an
    /// approximation). `p` is in [0.0, 100.0].
    pub fn percentile(&self, p: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target = ((p / 100.0) * self.count as f64).ceil() as u64;
        let target = target.max(1);
        let mut accumulated: u64 = 0;

        // Walk direct buckets
        for (i, &c) in self.direct.iter().enumerate() {
            accumulated += c;
            if accumulated >= target {
                return i as u64;
            }
        }

        // Walk overflow buckets — return the upper bound of each log range
        for i in 0..LOG_BUCKETS {
            accumulated += self.overflow[i];
            if accumulated >= target {
                return overflow_upper_bound(i);
            }
        }

        self.max
    }

    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        self.sum as f64 / self.count as f64
    }

    pub fn max(&self) -> u64 {
        if self.count == 0 { 0 } else { self.max }
    }

    pub fn min(&self) -> u64 {
        if self.count == 0 { 0 } else { self.min }
    }

    pub fn count(&self) -> u64 {
        self.count
    }
}

/// Map an overflow value (>= 4096) into a log bucket index 0..19.
fn log_bucket(cycles: u64) -> usize {
    // bucket 0  -> [4096, 8191]
    // bucket 1  -> [8192, 16383]
    // bucket k  -> [4096 << k, (4096 << (k+1)) - 1]
    let shifted = cycles >> 12; // divide by 4096
    let bits = 63 - shifted.leading_zeros(); // floor(log2(shifted))
    (bits as usize).min(LOG_BUCKETS - 1)
}

/// Upper bound of the range covered by overflow bucket `i`.
fn overflow_upper_bound(i: usize) -> u64 {
    if i >= LOG_BUCKETS - 1 {
        return u64::MAX;
    }
    // bucket i covers [4096 << i, (4096 << (i+1)) - 1]
    (DIRECT_BUCKETS as u64) << (i + 1)
}

// ---------------------------------------------------------------------------
// Serialisable result structs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct LatencyResults {
    pub cycles_per_op_p50: u64,
    pub cycles_per_op_p99: u64,
    pub cycles_per_op_p999: u64,
    pub cycles_per_op_p9999: u64,
    pub max: u64,
    pub min: u64,
    pub mean: f64,
    pub total_messages: u64,
}

#[derive(Debug, Serialize)]
pub struct SystemInfo {
    pub governor: String,
    pub turbo: String,
    pub isolcpus: String,
    pub tsc: String,
    pub kernel: String,
}

#[derive(Debug, Serialize)]
pub struct BenchResult {
    pub implementation: String,
    pub language: String,
    pub version: String,
    pub compiler: String,
    pub cpu: String,
    pub producer_core: usize,
    pub consumer_core: usize,
    pub capacity: usize,
    pub message_size_bytes: usize,
    pub warmup_messages: u64,
    pub measured_messages: u64,
    pub results: LatencyResults,
    pub system: SystemInfo,
}

impl BenchResult {
    /// Build a `BenchResult` from a filled histogram plus metadata.
    pub fn from_histogram(
        histogram: &CycleHistogram,
        implementation: &str,
        producer_core: usize,
        consumer_core: usize,
        capacity: usize,
        message_size_bytes: usize,
        warmup_messages: u64,
    ) -> Self {
        Self {
            implementation: implementation.to_string(),
            language: "Rust".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            compiler: rustc_version(),
            cpu: cpu_model(),
            producer_core,
            consumer_core,
            capacity,
            message_size_bytes,
            warmup_messages,
            measured_messages: histogram.count(),
            results: LatencyResults {
                cycles_per_op_p50: histogram.percentile(50.0),
                cycles_per_op_p99: histogram.percentile(99.0),
                cycles_per_op_p999: histogram.percentile(99.9),
                cycles_per_op_p9999: histogram.percentile(99.99),
                max: histogram.max(),
                min: histogram.min(),
                mean: histogram.mean(),
                total_messages: histogram.count(),
            },
            system: read_system_info(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialization cannot fail")
    }
}

// ---------------------------------------------------------------------------
// System introspection helpers
// ---------------------------------------------------------------------------

pub fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn cpu_model() -> String {
    #[cfg(target_os = "linux")]
    {
        read_proc_field("/proc/cpuinfo", "model name")
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        "unknown".to_string()
    }
}

pub fn read_system_info() -> SystemInfo {
    #[cfg(target_os = "linux")]
    {
        SystemInfo {
            governor: read_file_trimmed(
                "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            ),
            turbo: read_file_trimmed(
                "/sys/devices/system/cpu/intel_pstate/no_turbo",
            ),
            isolcpus: read_proc_field("/proc/cmdline", "isolcpus")
                .chars()
                .skip_while(|c| *c != '=')
                .skip(1)
                .take_while(|c| *c != ' ')
                .collect::<String>(),
            tsc: read_dmesg_tsc(),
            kernel: read_file_trimmed("/proc/version"),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        SystemInfo {
            governor: "n/a".to_string(),
            turbo: "n/a".to_string(),
            isolcpus: "n/a".to_string(),
            tsc: "n/a".to_string(),
            kernel: std::process::Command::new("uname")
                .arg("-a")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        }
    }
}

#[cfg(target_os = "linux")]
fn read_file_trimmed(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(target_os = "linux")]
fn read_proc_field(path: &str, field: &str) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| {
            contents
                .lines()
                .find(|l| l.contains(field))
                .map(|l| {
                    l.split(':')
                        .nth(1)
                        .unwrap_or(l)
                        .trim()
                        .to_string()
                })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(target_os = "linux")]
fn read_dmesg_tsc() -> String {
    std::process::Command::new("dmesg")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .find(|l| l.contains("tsc:"))
                .map(|l| l.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_empty() {
        let h = CycleHistogram::new();
        assert_eq!(h.count(), 0);
        assert_eq!(h.min(), 0);
        assert_eq!(h.max(), 0);
        assert_eq!(h.mean(), 0.0);
        assert_eq!(h.percentile(50.0), 0);
        assert_eq!(h.percentile(99.0), 0);
    }

    #[test]
    fn histogram_single_value() {
        let mut h = CycleHistogram::new();
        h.record(42);
        assert_eq!(h.count(), 1);
        assert_eq!(h.min(), 42);
        assert_eq!(h.max(), 42);
        assert_eq!(h.mean(), 42.0);
        assert_eq!(h.percentile(50.0), 42);
        assert_eq!(h.percentile(99.0), 42);
        assert_eq!(h.percentile(99.99), 42);
    }

    #[test]
    fn histogram_direct_range() {
        let mut h = CycleHistogram::new();
        // Record values 0..100 (one of each)
        for i in 0..100u64 {
            h.record(i);
        }
        assert_eq!(h.count(), 100);
        assert_eq!(h.min(), 0);
        assert_eq!(h.max(), 99);
        assert_eq!(h.percentile(50.0), 49);
        assert_eq!(h.percentile(100.0), 99);
        // p99 should be value 98 (99th of 100 values)
        assert_eq!(h.percentile(99.0), 98);
    }

    #[test]
    fn histogram_overflow_buckets() {
        let mut h = CycleHistogram::new();
        // Record a mix of direct and overflow values
        for i in 0..100u64 {
            h.record(i);
        }
        // Add some overflow values
        h.record(5000);
        h.record(10_000);
        h.record(100_000);
        assert_eq!(h.count(), 103);
        assert_eq!(h.min(), 0);
        assert_eq!(h.max(), 100_000);
        // p99.99 should land in overflow territory
        let p9999 = h.percentile(99.99);
        assert!(p9999 > 4095, "p99.99 should be in overflow range, got {p9999}");
    }
}
