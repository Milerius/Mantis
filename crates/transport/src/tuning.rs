//! CPU pinning and socket tuning helpers.

use tracing::{info, warn};

/// Socket-level tuning applied to each feed connection.
#[derive(Clone, Debug, Default)]
pub struct SocketTuning {
    /// Pin the feed thread to this logical core ID.
    pub core_id: Option<usize>,
    /// Enable `SO_BUSY_POLL` with this timeout in microseconds.
    /// Only effective on Linux with `CAP_NET_ADMIN` or `sysctl net.core.busy_poll`.
    #[cfg(feature = "tuning")]
    pub busy_poll_us: Option<u32>,
}

impl SocketTuning {
    /// Pin the current thread to the configured core.
    ///
    /// Logs a warning if pinning fails (non-fatal — thread continues on any core).
    pub fn apply_affinity(&self) {
        let Some(core_id) = self.core_id else {
            return;
        };

        let available = core_affinity::get_core_ids().unwrap_or_default();
        if let Some(id) = available.into_iter().find(|c| c.id == core_id) {
            if core_affinity::set_for_current(id) {
                info!(core = core_id, "thread pinned to core");
            } else {
                warn!(core = core_id, "failed to pin thread to core");
            }
        } else {
            warn!(core = core_id, "core ID not found in available cores");
        }
    }

    /// Apply `SO_BUSY_POLL` to a raw socket fd (Linux only).
    #[cfg(all(target_os = "linux", feature = "tuning"))]
    #[expect(unsafe_code, clippy::cast_possible_truncation)]
    pub fn apply_busy_poll(&self, fd: std::os::unix::io::RawFd) {
        let Some(us) = self.busy_poll_us else {
            return;
        };
        let val = us.cast_signed();
        let ret = unsafe {
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_BUSY_POLL,
                std::ptr::addr_of!(val).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if ret == 0 {
            info!(us, "SO_BUSY_POLL set");
        } else {
            warn!(
                us,
                errno = std::io::Error::last_os_error().raw_os_error(),
                "SO_BUSY_POLL failed"
            );
        }
    }
}
