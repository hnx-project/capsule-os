//! Raw PSCI 1.1 SMC / HVC dispatch.
//!
//! Used by `probe` (PSCI `AFFINITY_INFO` round trip) and `boot`
//! (`PSCI_CPU_ON` round trip).  Every public function returns the
//! raw SMC return value; callers map the value to `On, Off,
//! Pending, Invalid, ...` via `psci_ret_to_status()`.

use crate::arch::ArchHardware;

const PSCI_VERSION: u64 = 0x8400_0000;
const PSCI_CPU_ON: u64 = 0xC400_0003;
const PSCI_CPU_OFF: u64 = 0x8400_0002;
const PSCI_AFFINITY_INFO: u64 = 0x8000_0004;

/// Affinity state passed as the second argument to
/// `PSCI_AFFINITY_INFO`.  Only the `On` variant is needed at
/// probe time.
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum AffinityState {
    On = 0,
    Off = 1,
}

/// Convenience translation of `PSCI_VERSION` output.  BCD-ish:
/// minor in low byte, major in next.
#[inline]
pub fn psci_version() -> u32 {
    let raw: u64;
    unsafe {
        core::arch::asm!(
            "hvc #0",
            inout("x0") PSCI_VERSION => raw,
            out("x1") _, out("x2") _, out("x3") _,
            options(nostack)
        );
    }
    raw as u32
}

/// Issue `PSCI_CPU_ON(target_cpu, entry_pa, context_id)`.
/// Returns the raw 32-bit SMC return value (int32 sign-extended).
#[cfg(target_arch = "aarch64")]
pub fn psci_cpu_on(target_cpu: u64, entry_pa: u64, context_id: u64) -> i64 {
    let mut ret: u64;
    // Try HVC first (standard for `-M virt,secure=off`).
    unsafe {
        core::arch::asm!(
            "hvc #0",
            inout("x0") PSCI_CPU_ON => ret,
            in("x1") target_cpu,
            in("x2") entry_pa,
            in("x3") context_id,
            out("x4") _, out("x5") _, out("x6") _, out("x7") _,
        );
    }
    // DEN0037 §6.5: HVC returns negative as signed i32.  Mask to
    // i32 to mirror QEMU's behaviour cleanly.
    if (ret as i32) < 0 {
        ret as i32 as i64
    } else {
        ret as i64
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn psci_cpu_on(_t: u64, _e: u64, _c: u64) -> i64 {
    0
}

/// Issue `PSCI_CPU_OFF()`.  Never returns under normal use.
#[cfg(target_arch = "aarch64")]
pub fn psci_cpu_off() -> i64 {
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "hvc #0",
            inout("x0") PSCI_CPU_OFF => _,
            out("x1") _,
            inout("x2") 0u64 => ret,
            out("x3") _, out("x4") _, out("x5") _,
            out("x6") _, out("x7") _,
        );
    }
    ret as i64
}

#[cfg(not(target_arch = "aarch64"))]
pub fn psci_cpu_off() -> i64 {
    0
}

/// Issue `PSCI_AFFINITY_INFO(target_affinity, lowest_affinity_level)`.
/// `lowest_affinity_level` is typically 0 (= MPIDR_EL1.Aff0).
#[cfg(target_arch = "aarch64")]
pub fn psci_affinity_info(target_affinity: u64, state: AffinityState) -> i64 {
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "hvc #0",
            inout("x0") PSCI_AFFINITY_INFO => ret,
            in("x1") target_affinity,
            in("x2") state as u32 as u64,
            out("x3") _, out("x4") _, out("x5") _,
            out("x6") _, out("x7") _,
        );
    }
    ret as i64
}

#[cfg(not(target_arch = "aarch64"))]
pub fn psci_affinity_info(_a: u64, _s: AffinityState) -> i64 {
    0
}

/// Translates the PSCI return code for CPU_ON / AFFINITY_INFO into
/// a `CoreAlive` enum, never panicking on the unknown codes.
#[derive(Debug, PartialEq, Eq)]
pub enum CoreAlive {
    /// PSCI reports the core as up-and-running.
    Running,
    /// PSCI reports "on pending" — i.e. previous CPU_ON still
    /// being processed.  Treated as alive after we back off.
    Pending,
    /// PSCI rejects the request as the slot does not exist.
    Absent,
    /// Any other return code (already_on, denied, ...).  Treated
    /// as running so the orchestrator can fall through to a
    /// mailbox-based confirmation.
    Other,
}

pub fn psci_alive(ret: i64) -> CoreAlive {
    // ARM DEN0037: positive = success, negative = error code.
    match ret {
        0 => CoreAlive::Running,
        -2 => CoreAlive::Other, // ALREADY_ON
        -3 => CoreAlive::Pending,
        -4 => CoreAlive::Absent, // INVALID_PARAMS
        -5 => CoreAlive::Absent, // DENIED
        _ => CoreAlive::Other,
    }
}
