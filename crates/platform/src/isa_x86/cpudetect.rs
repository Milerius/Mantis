#![allow(unsafe_code)]
//! x86_64 CPUID feature detection with load-time caching.
//!
//! CPUID is ~70 cycles / ~120 latency. Results are cached in a static
//! `OnceLock`, initialized on first feature query.

use std::sync::OnceLock;

/// Raw CPUID register output.
struct CpuIdRegs {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
}

/// Execute CPUID instruction with given leaf (eax) and subleaf (ecx).
fn cpuid(leaf: u32, subleaf: u32) -> CpuIdRegs {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    // SAFETY: CPUID is available on all x86_64 CPUs and only reads CPU
    // information registers — it does not access memory or produce side effects.
    //
    // `rbx` is callee-saved and reserved by LLVM for internal use (PIC/GOT
    // addressing). We must save it ourselves: push rbx before the instruction,
    // copy the output to a different register, then restore rbx. The `mov`
    // transfers the EBX result into the scratch register the compiler allocated
    // for `ebx`, leaving the full RBX value intact across the asm block.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            inout("ecx") subleaf => ecx,
            ebx_out = out(reg) ebx,
            lateout("edx") edx,
            options(nostack, nomem),
        );
    }
    CpuIdRegs { eax, ebx, ecx, edx }
}

/// Returns the CPU brand string from CPUID extended leaves 0x80000002-4.
///
/// Returns `"unknown"` if extended CPUID is not supported.
#[must_use]
pub fn cpu_name_x86() -> std::string::String {
    use std::string::String;

    let ext = cpuid(0x8000_0000, 0);
    if ext.eax < 0x8000_0004 {
        return String::from("unknown");
    }

    let mut name = [0u8; 48];
    for (i, leaf) in (0x8000_0002u32..=0x8000_0004).enumerate() {
        let regs = cpuid(leaf, 0);
        let offset = i * 16;
        name[offset..offset + 4].copy_from_slice(&regs.eax.to_le_bytes());
        name[offset + 4..offset + 8].copy_from_slice(&regs.ebx.to_le_bytes());
        name[offset + 8..offset + 12].copy_from_slice(&regs.ecx.to_le_bytes());
        name[offset + 12..offset + 16].copy_from_slice(&regs.edx.to_le_bytes());
    }

    let len = name.iter().position(|&b| b == 0).unwrap_or(48);
    let s = String::from_utf8_lossy(&name[..len]);
    String::from(s.trim())
}

/// Cached CPU feature flags.
struct CpuFeatures {
    has_sse2: bool,
    has_sse3: bool,
    has_ssse3: bool,
    has_sse41: bool,
    has_sse42: bool,
    has_avx: bool,
    has_avx2: bool,
    has_avx512f: bool,
    has_avx512bw: bool,
    has_avx512dq: bool,
    has_avx512vl: bool,
    has_adx: bool,
    has_bmi1: bool,
    has_bmi2: bool,
    has_rdrand: bool,
    has_rdseed: bool,
    has_aes_ni: bool,
    has_pclmulqdq: bool,
}

static FEATURES: OnceLock<CpuFeatures> = OnceLock::new();

fn detect() -> &'static CpuFeatures {
    FEATURES.get_or_init(|| {
        // Leaf 1: ECX and EDX feature bits
        let leaf1 = cpuid(1, 0);
        // Leaf 7, subleaf 0: EBX and ECX feature bits
        let leaf7 = cpuid(7, 0);

        CpuFeatures {
            // Leaf 1 EDX
            has_sse2: leaf1.edx & (1 << 26) != 0,
            // Leaf 1 ECX
            has_sse3: leaf1.ecx & (1 << 0) != 0,
            has_ssse3: leaf1.ecx & (1 << 9) != 0,
            has_sse41: leaf1.ecx & (1 << 19) != 0,
            has_sse42: leaf1.ecx & (1 << 20) != 0,
            has_avx: leaf1.ecx & (1 << 28) != 0,
            has_aes_ni: leaf1.ecx & (1 << 25) != 0,
            has_pclmulqdq: leaf1.ecx & (1 << 1) != 0,
            has_rdrand: leaf1.ecx & (1 << 30) != 0,
            // Leaf 7 EBX
            has_avx2: leaf7.ebx & (1 << 5) != 0,
            has_bmi1: leaf7.ebx & (1 << 3) != 0,
            has_bmi2: leaf7.ebx & (1 << 8) != 0,
            has_adx: leaf7.ebx & (1 << 19) != 0,
            has_avx512f: leaf7.ebx & (1 << 16) != 0,
            has_avx512bw: leaf7.ebx & (1 << 30) != 0,
            has_avx512dq: leaf7.ebx & (1 << 17) != 0,
            has_avx512vl: leaf7.ebx & (1 << 31) != 0,
            has_rdseed: leaf7.ebx & (1 << 18) != 0,
        }
    })
}

/// SSE2 support (always true on x86_64).
#[must_use]
#[inline]
pub fn has_sse2() -> bool {
    detect().has_sse2
}

/// SSE3 support.
#[must_use]
#[inline]
pub fn has_sse3() -> bool {
    detect().has_sse3
}

/// SSSE3 support.
#[must_use]
#[inline]
pub fn has_ssse3() -> bool {
    detect().has_ssse3
}

/// SSE4.1 support.
#[must_use]
#[inline]
pub fn has_sse41() -> bool {
    detect().has_sse41
}

/// SSE4.2 support.
#[must_use]
#[inline]
pub fn has_sse42() -> bool {
    detect().has_sse42
}

/// AVX support.
#[must_use]
#[inline]
pub fn has_avx() -> bool {
    detect().has_avx
}

/// AVX2 support.
#[must_use]
#[inline]
pub fn has_avx2() -> bool {
    detect().has_avx2
}

/// AVX-512 Foundation support.
#[must_use]
#[inline]
pub fn has_avx512f() -> bool {
    detect().has_avx512f
}

/// AVX-512 Byte/Word support.
#[must_use]
#[inline]
pub fn has_avx512bw() -> bool {
    detect().has_avx512bw
}

/// AVX-512 Doubleword/Quadword support.
#[must_use]
#[inline]
pub fn has_avx512dq() -> bool {
    detect().has_avx512dq
}

/// AVX-512 Vector Length support.
#[must_use]
#[inline]
pub fn has_avx512vl() -> bool {
    detect().has_avx512vl
}

/// ADX (multi-precision add-carry) support.
#[must_use]
#[inline]
pub fn has_adx() -> bool {
    detect().has_adx
}

/// BMI1 (bit manipulation) support.
#[must_use]
#[inline]
pub fn has_bmi1() -> bool {
    detect().has_bmi1
}

/// BMI2 (bit manipulation) support.
#[must_use]
#[inline]
pub fn has_bmi2() -> bool {
    detect().has_bmi2
}

/// RDRAND hardware RNG support.
#[must_use]
#[inline]
pub fn has_rdrand() -> bool {
    detect().has_rdrand
}

/// RDSEED hardware RNG support.
#[must_use]
#[inline]
pub fn has_rdseed() -> bool {
    detect().has_rdseed
}

/// AES-NI support.
#[must_use]
#[inline]
pub fn has_aes_ni() -> bool {
    detect().has_aes_ni
}

/// PCLMULQDQ (carry-less multiplication) support.
#[must_use]
#[inline]
pub fn has_pclmulqdq() -> bool {
    detect().has_pclmulqdq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_name_x86_nonempty() {
        assert!(!cpu_name_x86().is_empty());
    }

    #[test]
    fn has_sse2_is_true() {
        // SSE2 is mandatory on all x86_64 CPUs.
        assert!(has_sse2());
    }

    #[test]
    fn features_dont_panic() {
        let _ = has_sse2();
        let _ = has_sse3();
        let _ = has_ssse3();
        let _ = has_sse41();
        let _ = has_sse42();
        let _ = has_avx();
        let _ = has_avx2();
        let _ = has_avx512f();
        let _ = has_avx512bw();
        let _ = has_avx512dq();
        let _ = has_avx512vl();
        let _ = has_adx();
        let _ = has_bmi1();
        let _ = has_bmi2();
        let _ = has_rdrand();
        let _ = has_rdseed();
        let _ = has_aes_ni();
        let _ = has_pclmulqdq();
    }

    #[test]
    fn feature_detection_is_consistent() {
        let sse2_a = has_sse2();
        let avx_a = has_avx();
        let avx2_a = has_avx2();
        let bmi1_a = has_bmi1();

        let sse2_b = has_sse2();
        let avx_b = has_avx();
        let avx2_b = has_avx2();
        let bmi1_b = has_bmi1();

        assert_eq!(sse2_a, sse2_b);
        assert_eq!(avx_a, avx_b);
        assert_eq!(avx2_a, avx2_b);
        assert_eq!(bmi1_a, bmi1_b);
    }
}
