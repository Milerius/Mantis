//! CPU identification.
//!
//! Detects the CPU model name at runtime via platform-specific APIs.

/// Returns the CPU model name, or `"unknown"` if detection fails.
#[must_use]
pub fn cpu_name() -> std::string::String {
    cpu_name_impl()
}

#[cfg(target_os = "macos")]
fn cpu_name_impl() -> std::string::String {
    std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .and_then(|o| std::string::String::from_utf8(o.stdout).ok())
        .map_or_else(
            || std::string::String::from("unknown"),
            |s| std::string::String::from(s.trim()),
        )
}

#[cfg(target_os = "linux")]
fn cpu_name_impl() -> std::string::String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|n| std::string::String::from(n.trim()))
        })
        .unwrap_or_else(|| std::string::String::from("unknown"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cpu_name_impl() -> std::string::String {
    std::string::String::from("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_name_returns_nonempty() {
        assert!(!cpu_name().is_empty());
    }
}
