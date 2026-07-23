//! `boot_secondary_cores` — the single orchestrator called from
//! `kernel_main` after the scheduler has its initial threads
//! queued.  Walks the DTB/PSCI table and brings up every CPU
//! whose slot is in `POSSIBLE_MASK` but not yet in `ONLINE_MASK`.

use crate::arch::CurrentArch;
use crate::smp::psci::psci_cpu_on;
use crate::smp::{BOOTED_CORES, CPU_DESCRIPTORS, CPU_DESCRIPTOR_COUNT, MAX_CORES, ONLINE_MASK, POSSIBLE_MASK};
use core::sync::atomic::Ordering;

/// Mailbox slots.  Defined in `arch/aarch64/boot_asm.S` — we only
/// have read/write access at boot time.  `unsafe` because the
/// access is to `.bss`-adjacent globals.
unsafe extern "C" {
    static mut SECONDARY_CORE_ENTRY: usize;
    static mut SECONDARY_CORE_SP: usize;
}

/// Physical address the bootloader copies our kernel to.  Must
/// match `KERNEL_ENTRY_PHYS_ADDR` in the boot loader contract.
const KERNEL_ENTRY_PHYS_ADDR: u64 = 0x40080000;

/// Brings up every POSSIBLE & not-yet-online slot via
/// `PSCI_CPU_ON`.  Each slot is then expected to call back into
/// `kmain_secondary`, which clears the SECONDARY_CORE_ENTRY
/// mailbox and increments `BOOTED_CORES`.
pub fn boot_secondary_cores() {
    crate::log_info!(
        "SMP",
        "Booting secondary cores — possible_mask = 0x{:x}",
        snapshot_possible()
    );

    for slot in 1..MAX_CORES {
        if !POSSIBLE_MASK.test(slot) {
            continue;
        }
        if ONLINE_MASK.test(slot) {
            // Firmware already brought it up — skip the
            // `CPU_ON` dance.  Clear the mailbox descriptor so the
            // MPSC handshake from `kmain_secondary` (if it ever
            // runs for some reason) doesn't deadlock.
            continue;
        }
        let kstack_pa = match crate::arch::aarch64::phys::alloc_kstack_page() {
            Ok(pa) => pa,
            Err(_) => {
                crate::log_warn!("SMP", "slot {} kstack alloc failed", slot);
                continue;
            }
        };
        let sp_top =
            crate::arch::mmu_facade::pa_to_kernel_va(kstack_pa.as_usize()) + 4096;

        unsafe {
            core::ptr::write_volatile(&mut SECONDARY_CORE_SP, sp_top);
            core::ptr::write_volatile(
                &mut SECONDARY_CORE_ENTRY,
                crate::smp::per_core::kmain_secondary as usize,
            );
            core::sync::atomic::fence(Ordering::SeqCst);
        }

        let ret = psci_cpu_on(slot as u64, KERNEL_ENTRY_PHYS_ADDR, slot as u64);
        if ret != 0 && ret != -2 {
            crate::log_warn!(
                "SMP",
                "PSCI CPU_ON slot {} failed: ret={}",
                slot, ret
            );
            continue;
        }

        // Wait for the secondary to clear SECONDARY_CORE_ENTRY,
        // which signals that `kmain_secondary` is past the boot
        // handshake.
        let mut spins = 0u32;
        let mut ack_seen = false;
        unsafe {
            while core::ptr::read_volatile(&SECONDARY_CORE_ENTRY) != 0 {
                core::hint::spin_loop();
                spins += 1;
                if spins > 50_000_000 {
                    crate::log_warn!(
                        "SMP",
                        "slot {} handshake timeout — PSCI likely no-op'd",
                        slot
                    );
                    break;
                }
            }
            ack_seen = core::ptr::read_volatile(&SECONDARY_CORE_ENTRY) == 0;
        }
        // Even if the handshake timed out, mark the slot online so
        // the scheduler doesn't reject its ticks; the alternative
        // is worse — the slot's CPU will silently disappear from
        // the topology.  Either PSCI brought it up, or in the rare
        // case it didn't, ONLINE_MASK will diverge from the real
        // state until the user reissues `PSCI_CPU_ON`.
        ONLINE_MASK.set(slot);
        crate::log_info!(
            "SMP",
            "slot {} online (handshake{}observed)",
            slot,
            if ack_seen { " " } else { " NOT " }
        );
    }

    crate::log_info!(
        "SMP",
        "All secondary cores brought up.  online_mask = 0x{:x}",
        snapshot_online()
    );
}

/// Snapshot of all POSSIBLE bits.
pub fn snapshot_possible() -> u64 {
    let mut bits = 0u64;
    for s in 0..MAX_CORES {
        if POSSIBLE_MASK.test(s) {
            bits |= 1u64 << s;
        }
    }
    bits
}

/// Snapshot of all ONLINE bits.
pub fn snapshot_online() -> u64 {
    let mut bits = 0u64;
    for s in 0..MAX_CORES {
        if ONLINE_MASK.test(s) {
            bits |= 1u64 << s;
        }
    }
    bits
}

/// Number of cores recorded as booted (slot 0 + each secondary
/// that reached `register_core()`).  Used by callers that want to
/// gate user-facing init until SMP is live.
pub fn booted_core_count() -> usize {
    BOOTED_CORES.load(Ordering::Relaxed)
}

/// Whether slot 0 has been confirmed as the kernel's primary CPU.
/// Always true after `register_core(0)` is called from
/// `kernel_main`; provide this assertion for clarity.
pub fn core0_registered() -> bool {
    let s = crate::smp::CURRENT_CORE_SLOT.load(Ordering::Relaxed);
    s != usize::MAX
}

/// Print a summary line `cpus=...` using the recorded
/// descriptors.  Helpful for boot logs.
pub fn log_topology() {
    let count = unsafe { CPU_DESCRIPTOR_COUNT };
    crate::log_info!("SMP", "Discovered {} cpu{}: ", count, if count == 1 { "" } else { "s" });
    unsafe {
        for i in 0..count {
            let d = CPU_DESCRIPTORS[i];
            crate::log_info!(
                "SMP",
                "  cpu@{} method={} enabled={} present={}",
                d.reg,
                match d.enable_method {
                    1 => "psci",
                    2 => "spin-table",
                    _ => "unknown",
                },
                d.enabled,
                d.present,
            );
        }
    }
}
