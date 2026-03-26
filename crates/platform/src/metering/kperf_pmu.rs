#![expect(
    unsafe_code,
    reason = "FFI calls to kperf/kperfdata private frameworks via dlopen"
)]
//! macOS ARM64 hardware counters via kperf/kperfdata private frameworks.
//!
//! Reads instructions retired and branch misses using Apple's private
//! kperf PMU interface. Requires root privileges; `try_new()` fails
//! gracefully without them.
//!
//! L1D and LLC cache counters are not available via kperf on Apple
//! Silicon and always return 0 in the deltas.

extern crate std;

use std::ffi::{c_char, c_int, c_void, CStr};
use std::format;
use std::io;
use std::ptr;

use super::hw_counters::{HwCounterDeltas, HwCounters};

/// Maximum number of counters readable in one `kpc_get_thread_counters` call.
const KPC_MAX_COUNTERS: usize = 32;

/// `KPC_MAX_COUNTERS` as `u32` for FFI calls that take a count parameter.
const KPC_MAX_COUNTERS_U32: u32 = 32;

const KPC_CLASS_CONFIGURABLE_MASK: u32 = 1 << 1;
const RTLD_LAZY: c_int = 1;

const KPERF_PATH: &CStr =
    c"/System/Library/PrivateFrameworks/kperf.framework/kperf";
const KPERFDATA_PATH: &CStr =
    c"/System/Library/PrivateFrameworks/kperfdata.framework/kperfdata";

/// Event name fallback chains from `/usr/share/kpep/<cpu>.plist`.
const INSTRUCTIONS_NAMES: &[&CStr] = &[
    c"FIXED_INSTRUCTIONS", // Apple A7-A17, M1-M4
    c"INST_RETIRED.ANY",   // Intel fallback
];

const BRANCH_MISS_NAMES: &[&CStr] = &[
    c"BRANCH_MISPRED_NONSPEC",       // Apple A7-A17 (macOS 12+)
    c"BRANCH_MISPREDICT",            // Apple A7-A14 (older name)
    c"BR_MISP_RETIRED.ALL_BRANCHES", // Intel Core 2nd-10th gen
    c"BR_INST_RETIRED.MISPRED",      // Intel Yonah, Merom
];

/// All event alias chains, in order: [0]=instructions, [1]=branch misses.
const EVENT_ALIASES: [&[&CStr]; 2] = [INSTRUCTIONS_NAMES, BRANCH_MISS_NAMES];

// ---------------------------------------------------------------------------
// FFI type aliases (must precede statements per clippy::items_after_statements)
// ---------------------------------------------------------------------------

// kperf function signatures
type FnCtrsGet = unsafe extern "C" fn(*mut c_int) -> c_int;
type FnCtrsSet = unsafe extern "C" fn(c_int) -> c_int;
type FnSetCfg = unsafe extern "C" fn(u32, *mut u64) -> c_int;
type FnSetU32 = unsafe extern "C" fn(u32) -> c_int;
type FnThreadCtrs = unsafe extern "C" fn(u32, u32, *mut u64) -> c_int;

// kperfdata function signatures
type FnDbCreate =
    unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> c_int;
type FnDbEvent = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *mut *mut c_void,
) -> c_int;
type FnCfgCreate =
    unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int;
type FnCfgAddEv = unsafe extern "C" fn(
    *mut c_void,
    *mut *mut c_void,
    u32,
    *mut u32,
) -> c_int;
type FnCfgVoid = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnCfgBuf =
    unsafe extern "C" fn(*mut c_void, *mut u64, usize) -> c_int;
type FnCfgUsize =
    unsafe extern "C" fn(*mut c_void, *mut usize) -> c_int;
type FnCfgU32 =
    unsafe extern "C" fn(*mut c_void, *mut u32) -> c_int;
type FnCfgMap =
    unsafe extern "C" fn(*mut c_void, *mut usize, usize) -> c_int;

// ---------------------------------------------------------------------------
// FFI: dlopen / dlsym
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// Load a typed function pointer from a `dlopen` handle.
///
/// # Safety
///
/// `handle` must be a valid `dlopen` handle. The caller must ensure that
/// `T` matches the actual signature of the symbol named `name`.
unsafe fn load_sym<T: Copy>(
    handle: *mut c_void,
    name: &CStr,
) -> Result<T, io::Error> {
    // SAFETY: handle is a valid dlopen handle, name is a null-terminated
    // C string pointing to a symbol in the loaded framework.
    let ptr = unsafe { dlsym(handle, name.as_ptr()) };
    if ptr.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("kperf symbol not found: {}", name.to_string_lossy()),
        ));
    }
    // SAFETY: ptr is non-null and points to a valid function. Function
    // pointers and data pointers have the same size on macOS ARM64.
    Ok(unsafe { std::mem::transmute_copy(&ptr) })
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Grouped hardware counters via macOS `kperf` / `kperfdata`.
///
/// Reads 2 counters per measurement: instructions retired and branch
/// misses. The PMU configuration is programmed once in [`try_new`] and
/// stays active for the process lifetime. Counter values are read
/// per-thread via `kpc_get_thread_counters`.
///
/// [`try_new`]: Self::try_new
pub struct KperfPmuCounters {
    /// `kpc_get_thread_counters(tid, buf_count, buf) -> c_int`
    get_thread_counters: FnThreadCtrs,
    /// Index into the counter array for instructions retired.
    instructions_idx: usize,
    /// Index into the counter array for branch misses.
    branch_misses_idx: usize,
}

/// Snapshot of counter values at measurement start.
pub struct KperfSnapshot {
    instructions: u64,
    branch_misses: u64,
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

impl KperfPmuCounters {
    /// Try to create kperf PMU counters.
    ///
    /// Programs the Apple Silicon PMU to count instructions retired and
    /// branch misses. The frameworks are loaded via `dlopen`; the PMU
    /// database for the current CPU is opened automatically.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if:
    /// - Private frameworks cannot be loaded.
    /// - Root privileges are missing (`kpc_force_all_ctrs` denied).
    /// - The PMU database lacks the requested events.
    #[expect(clippy::too_many_lines, reason = "linear FFI init sequence")]
    pub fn try_new() -> Result<Self, io::Error> {
        // SAFETY: all FFI calls in this block target Apple private
        // frameworks loaded via dlopen. Each symbol's type matches the
        // reverse-engineered signature from XNU headers. Pointers
        // received from kpep_* calls are opaque and only passed back
        // into the same framework.
        unsafe {
            // --- Load frameworks ---
            let kperf = dlopen(KPERF_PATH.as_ptr(), RTLD_LAZY);
            if kperf.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "cannot load kperf.framework",
                ));
            }
            let kperfdata = dlopen(KPERFDATA_PATH.as_ptr(), RTLD_LAZY);
            if kperfdata.is_null() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "cannot load kperfdata.framework",
                ));
            }

            // --- Load kperf symbols ---
            let kpc_force_all_ctrs_get: FnCtrsGet =
                load_sym(kperf, c"kpc_force_all_ctrs_get")?;
            let kpc_force_all_ctrs_set: FnCtrsSet =
                load_sym(kperf, c"kpc_force_all_ctrs_set")?;
            let kpc_set_config: FnSetCfg =
                load_sym(kperf, c"kpc_set_config")?;
            let kpc_set_counting: FnSetU32 =
                load_sym(kperf, c"kpc_set_counting")?;
            let kpc_set_thread_counting: FnSetU32 =
                load_sym(kperf, c"kpc_set_thread_counting")?;
            let get_thread_counters: FnThreadCtrs =
                load_sym(kperf, c"kpc_get_thread_counters")?;

            // --- Load kperfdata symbols ---
            let kpep_db_create: FnDbCreate =
                load_sym(kperfdata, c"kpep_db_create")?;
            let kpep_db_event: FnDbEvent =
                load_sym(kperfdata, c"kpep_db_event")?;
            let kpep_config_create: FnCfgCreate =
                load_sym(kperfdata, c"kpep_config_create")?;
            let kpep_config_add_event: FnCfgAddEv =
                load_sym(kperfdata, c"kpep_config_add_event")?;
            let kpep_config_force_counters: FnCfgVoid =
                load_sym(kperfdata, c"kpep_config_force_counters")?;
            let kpep_config_kpc: FnCfgBuf =
                load_sym(kperfdata, c"kpep_config_kpc")?;
            let kpep_config_kpc_count: FnCfgUsize =
                load_sym(kperfdata, c"kpep_config_kpc_count")?;
            let kpep_config_kpc_classes: FnCfgU32 =
                load_sym(kperfdata, c"kpep_config_kpc_classes")?;
            let kpep_config_kpc_map: FnCfgMap =
                load_sym(kperfdata, c"kpep_config_kpc_map")?;

            // --- Check permission (requires root) ---
            let mut force_ctrs: c_int = 0;
            if kpc_force_all_ctrs_get(
                &raw mut force_ctrs,
            ) != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "kpc_force_all_ctrs_get failed (root required)",
                ));
            }

            // --- Open PMC database for current CPU ---
            let mut db: *mut c_void = ptr::null_mut();
            if kpep_db_create(ptr::null(), &raw mut db) != 0 {
                return Err(io::Error::other(
                    "kpep_db_create failed",
                ));
            }

            // --- Create kpep config ---
            let mut cfg: *mut c_void = ptr::null_mut();
            if kpep_config_create(db, &raw mut cfg) != 0 {
                return Err(io::Error::other(
                    "kpep_config_create failed",
                ));
            }
            if kpep_config_force_counters(cfg) != 0 {
                return Err(io::Error::other(
                    "kpep_config_force_counters failed",
                ));
            }

            // --- Find events with fallback chains ---
            let mut ev_ptrs: [*mut c_void; 2] = [ptr::null_mut(); 2];
            for (i, names) in EVENT_ALIASES.iter().enumerate() {
                for name in *names {
                    let mut ev: *mut c_void = ptr::null_mut();
                    if kpep_db_event(db, name.as_ptr(), &raw mut ev)
                        == 0
                    {
                        ev_ptrs[i] = ev;
                        break;
                    }
                }
                if ev_ptrs[i].is_null() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "kperf: no event found for alias group {i}"
                        ),
                    ));
                }
            }

            // --- Add events to config ---
            for ev_ptr in &mut ev_ptrs {
                if kpep_config_add_event(
                    cfg,
                    ev_ptr,
                    0,
                    ptr::null_mut(),
                ) != 0
                {
                    return Err(io::Error::other(
                        "kpep_config_add_event failed",
                    ));
                }
            }

            // --- Extract KPC configuration ---
            let mut classes: u32 = 0;
            let mut reg_count: usize = 0;
            let mut counter_map = [0usize; KPC_MAX_COUNTERS];
            let mut regs = [0u64; KPC_MAX_COUNTERS];

            let map_size = std::mem::size_of_val(&counter_map);
            let regs_size = std::mem::size_of_val(&regs);

            if kpep_config_kpc_classes(cfg, &raw mut classes) != 0
                || kpep_config_kpc_count(cfg, &raw mut reg_count) != 0
                || kpep_config_kpc_map(
                    cfg,
                    counter_map.as_mut_ptr(),
                    map_size,
                ) != 0
                || kpep_config_kpc(cfg, regs.as_mut_ptr(), regs_size)
                    != 0
            {
                return Err(io::Error::other(
                    "kpep_config_kpc setup failed",
                ));
            }

            // --- Program kernel PMU ---
            if kpc_force_all_ctrs_set(1) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "kpc_force_all_ctrs_set failed (root required)",
                ));
            }
            if (classes & KPC_CLASS_CONFIGURABLE_MASK) != 0
                && reg_count > 0
                && kpc_set_config(classes, regs.as_mut_ptr()) != 0
            {
                return Err(io::Error::other("kpc_set_config failed"));
            }
            if kpc_set_counting(classes) != 0
                || kpc_set_thread_counting(classes) != 0
            {
                return Err(io::Error::other(
                    "kpc_set_counting failed",
                ));
            }

            // --- Validate counter_map indices ---
            let instructions_idx = counter_map[0];
            let branch_misses_idx = counter_map[1];
            if instructions_idx >= KPC_MAX_COUNTERS
                || branch_misses_idx >= KPC_MAX_COUNTERS
            {
                return Err(io::Error::other(
                    "invalid counter_map indices from kpep",
                ));
            }

            Ok(Self {
                get_thread_counters,
                instructions_idx,
                branch_misses_idx,
            })
        }
    }

    /// Read raw thread counter values into a buffer.
    fn read_counters(&self) -> Option<[u64; KPC_MAX_COUNTERS]> {
        let mut buf = [0u64; KPC_MAX_COUNTERS];
        // SAFETY: buf is a valid KPC_MAX_COUNTERS-element array.
        // kpc_get_thread_counters reads the calling thread's counters.
        let ret = unsafe {
            (self.get_thread_counters)(
                0,
                KPC_MAX_COUNTERS_U32,
                buf.as_mut_ptr(),
            )
        };
        if ret != 0 {
            return None;
        }
        Some(buf)
    }
}

// ---------------------------------------------------------------------------
// HwCounters implementation
// ---------------------------------------------------------------------------

impl HwCounters for KperfPmuCounters {
    type Snapshot = KperfSnapshot;

    fn start(&self) -> Option<Self::Snapshot> {
        let counters = self.read_counters()?;
        Some(KperfSnapshot {
            instructions: counters[self.instructions_idx],
            branch_misses: counters[self.branch_misses_idx],
        })
    }

    fn read(
        &self,
        snapshot: &Option<Self::Snapshot>,
    ) -> Option<HwCounterDeltas> {
        let base = snapshot.as_ref()?;
        let counters = self.read_counters()?;
        Some(HwCounterDeltas {
            instructions: counters[self.instructions_idx]
                .saturating_sub(base.instructions),
            branch_misses: counters[self.branch_misses_idx]
                .saturating_sub(base.branch_misses),
            l1d_misses: 0,
            llc_misses: 0,
        })
    }
}
