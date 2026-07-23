//! SMP (Symmetric Multiprocessing) bootstrap & topology discovery.
//!
//! This module replaces the original "1 file, 1 PSCI loop" boot with
//! a UNIX-style, FDT-driven topology bring-up.  Each secondary core
//! is launched by `boot_secondary_cores()`, which:
//!
//!   1. Reads `/cpus` from the DTB provided by the bootloader.
//!   2. Cross-checks the firmware declaration against an explicit
//!      `PSCI_AFFINITY_INFO` round trip.
//!   3. For each present/online slot, allocates a private kernel
//!      stack, writes the mailbox, and issues `PSCI_CPU_ON`.
//!
//! Submodules:
//!   - `psci`       – Direct PSCI SMC/HVC dispatch
//!   - `probe`      – FDT `/cpus` scan + ONLINE_MASK population
//!   - `per_core`   – Secondary entry points (`kmain_secondary`)
//!   - `boot`       – `boot_secondary_cores()` orchestrator

pub mod psci;
pub mod probe;
pub mod per_core;
pub mod boot;

use crate::fdt::CpuMask;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Maximum number of CPU slots tracked by the kernel.  The DTB scan
/// itself may return up to `crate::fdt::MAX_CPUS_IN_DTB` (= 16)
/// entries; this limit is the absolute upper bound for any
/// per-CPU static array (`Scheduler.current_indices`,
/// `idle_flags`, etc).
pub const MAX_CORES: usize = 8;

/// `possible`: bits set if the firmware declares the slot as a
/// `cpu@N` in `/cpus`.  Always treated as "this slot is real
/// hardware; you may try to bring it up".
pub static POSSIBLE_MASK: CpuMask = CpuMask::new();

/// `present`: a subset of `POSSIBLE_MASK` reflecting cpus that pass
/// any firmware-level sanity checks (compatible string contains
/// "arm,cortex", etc.).  Reserved for future use; Pangu 1.0 does
/// not gate scheduling on this bit.
pub static PRESENT_MASK: CpuMask = CpuMask::new();

/// `online`: a subset of `POSSIBLE_MASK` whose slot has been
/// confirmed running either by `PSCI_AFFINITY_INFO` or by the
/// entry handshake in `kmain_secondary`.  The scheduler's
/// `current_indices[]` is only meaningful when the bit is set.
pub static ONLINE_MASK: CpuMask = CpuMask::new();

/// Cached `cpu@N` descriptors produced by `probe::probe_cpus()`.
/// Indexed by slot; only `0..cpu_descriptors.count` is valid.
pub static mut CPU_DESCRIPTORS: [CpuDescStatic; 16] = [const {
    CpuDescStatic {
        reg: 0,
        enable_method: 0,
        enabled: false,
        present: false,
    }
}; 16];
pub static mut CPU_DESCRIPTOR_COUNT: usize = 0;

/// Compact snapshot of `/cpus` parsed data, kept around because
/// `heapless::String` is not `Sync` and `static mut` arrays cannot
/// hold heapless types without `MaybeUninit`.
#[derive(Debug, Clone, Copy)]
pub struct CpuDescStatic {
    pub reg: u32,
    /// 4-byte tag of the enable-method (hashed to avoid string in
    /// `static mut`); canonical values:
    /// 0 = none, 1 = "psci", 2 = "spin-table".
    pub enable_method: u8,
    pub enabled: bool,
    pub present: bool,
}

/// Holds the slot id of the current CPU.  `usize::MAX` means
/// "the kernel hasn't recorded a slot yet" (very early boot) — in
/// that case `current_core_id()` falls back to `MPIDR_EL1`.
pub static CURRENT_CORE_SLOT: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Returns the per-CPU slot id of the caller (0..MAX_CORES).
///
/// Resolution order:
///   1. `CURRENT_CORE_SLOT` (cheap atom load) — set as soon as a
///      secondary core clears the boot mailbox, or by `kernel_main`
///      for core 0.
///   2. `MPIDR_EL1` (architectural fallback) — used during the
///      few instructions before the slot is recorded.
///
/// The caller is expected to compare the result against `MAX_CORES`
/// before using it as an array index; out-of-range values are
/// treated as "this CPU is not under kernel control yet" by the
/// scheduler.
#[inline]
pub fn current_core_id() -> usize {
    let slot = CURRENT_CORE_SLOT.load(Ordering::Relaxed);
    if slot != usize::MAX {
        return slot;
    }
    // Fallback: read MPIDR_EL1.Aff0.  On QEMU virt / Cortex-A72 this
    // is a contiguous 0..3 assignment that matches `reg` directly,
    // which is good enough for early boot and for cores that haven't
    // recorded a slot yet.
    #[cfg(target_arch = "aarch64")]
    {
        let mpidr: u64;
        unsafe {
            core::arch::asm!(
                "mrs {0}, mpidr_el1",
                out(reg) mpidr,
                options(nomem, preserves_flags)
            );
        }
        (mpidr & 0xff) as usize
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        0
    }
}

/// Hash a small set of well-known `enable-method` strings into a
/// `u8` so we can keep descriptors in `static mut`.
pub fn encode_enable_method(s: &str) -> u8 {
    match s.trim_end_matches('\0') {
        "" | "spintable" => 2,
        "psci" => 1,
        _ => 0,
    }
}

/// Number of CPUs that have already entered the kernel (recorded
/// via `register_core`).  Increments monotonically as each
/// secondary core clears the mailbox.
pub static BOOTED_CORES: AtomicUsize = AtomicUsize::new(1);

/// Counts secondary entry completions for the diagnostic log.
/// Increments inside `per_core::kmain_secondary` after the per-core
/// init has finished — if this stays at zero, the PSCI CPU_ON
/// handshake never actually delivered execution to the secondaries.
pub static SECONDARY_ENTRY_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Called by a secondary core (or by `boot_secondary_cores()`
/// itself for slot 0) once it has cleared the boot mailbox and
/// owns its own kernel stack.  Stores its slot in
/// `CURRENT_CORE_SLOT` and increments `BOOTED_CORES`.
pub fn register_core(slot: usize) {
    CURRENT_CORE_SLOT.store(slot, Ordering::Relaxed);
    BOOTED_CORES.fetch_add(1, Ordering::Relaxed);
    // The slot itself is now confirmed running: explicitly mark
    // ONLINE in case the probe didn't already do so via PSCI.
    ONLINE_MASK.set(slot);
}
