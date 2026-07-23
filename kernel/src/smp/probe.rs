//! Probe the DTB `/cpus` subtree and seed
//! `POSSIBLE_MASK` / `PRESENT_MASK` / `ONLINE_MASK`.
//!
//! Two-pass strategy:
//!   1. `pass_fdt()` — read `/cpus`, mark POSSIBLE slots, harvest
//!      descriptors.
//!   2. `pass_psci()` — for each POSSIBLE slot, ask PSCI whether
//!      the core is currently ON.  This catches cases where firmware
//!      booted core 1 first, then handed control to core 0; without
//!      this round trip we'd pretend core 1 was offline and try to
//!      restart it, getting `ALREADY_ON` from PSCI.
//!
//! If the DTB is missing or malformed we fall back to a single core
//! (slot 0) so the kernel still boots on stub platforms.

use crate::fdt::{scan_cpus_node, CpuDescriptor};
use crate::smp::{
    psci::{psci_affinity_info, psci_alive, AffinityState, CoreAlive},
    encode_enable_method, CpuDescStatic, CPU_DESCRIPTORS, CPU_DESCRIPTOR_COUNT,
    POSSIBLE_MASK, PRESENT_MASK, MAX_CORES,
};

/// Probe the DTB + PSCI and seed `POSSIBLE_MASK` / `PRESENT_MASK`
/// (leaves `ONLINE_MASK` final state to `boot::boot_secondary_cores`,
/// because an OFF -> ON transition can happen for slots that were
/// absent at probe time).
pub fn probe_cpus() {
    pass_fdt();
    pass_psci();
    log_summary();
}

fn pass_fdt() {
    let dtb = unsafe { crate::DTB_POINTER };
    if dtb.is_null() {
        // No DTB at all — only slot 0 is even theoretically
        // available.  This is the FDT-parse-failed fallback path
        // in `kernel_main` and the boot stub platform.
        POSSIBLE_MASK.set(0);
        PRESENT_MASK.set(0);
        record_descriptor(0, 0, true, true);
        crate::log_warn!("SMP", "no DTB; POSSIBLE=0x01");
        return;
    }

    let scan = match scan_cpus_node(dtb) {
        Ok(s) => s,
        Err(e) => {
            crate::log_warn!("SMP", "scan_cpus_node failed: {}; POSSIBLE=0x01", e);
            POSSIBLE_MASK.set(0);
            PRESENT_MASK.set(0);
            record_descriptor(0, 0, true, true);
            return;
        }
    };

    if scan.count == 0 {
        // DTB parsed but `/cpus` had no `device_type=cpu`.  Same
        // fallback as above.
        crate::log_warn!("SMP", "/cpus yielded 0 entries; POSSIBLE=0x01");
        POSSIBLE_MASK.set(0);
        PRESENT_MASK.set(0);
        record_descriptor(0, 0, true, true);
        return;
    }

    for i in 0..scan.count {
        let desc: CpuDescriptor = scan.cpus[i].clone();
        let slot = desc.reg as usize;
        if slot >= MAX_CORES {
            crate::log_warn!(
                "SMP",
                "cpu@{} skipped: slot {} >= MAX_CORES {}",
                desc.reg, slot, MAX_CORES
            );
            continue;
        }
        POSSIBLE_MASK.set(slot);
        let mut present = true;
        if !desc.compatible.is_empty()
            && !desc.compatible.contains("arm,cortex")
        {
            present = false;
        }
        if present {
            PRESENT_MASK.set(slot);
        }
        let method_tag = encode_enable_method(desc.enable_method.as_str());
        record_descriptor(slot, method_tag, desc.enabled, present);
    }
}

fn pass_psci() {
    for slot in 0..MAX_CORES {
        if !POSSIBLE_MASK.test(slot) {
            continue;
        }
        let affinity = slot as u64;
        let ret = psci_affinity_info(affinity, AffinityState::On);
        match psci_alive(ret) {
            CoreAlive::Running | CoreAlive::Other => {
                crate::smp::ONLINE_MASK.set(slot);
            }
            CoreAlive::Pending => {
                // Quiesce for a bit then retry once.
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
                let ret2 = psci_affinity_info(affinity, AffinityState::On);
                if matches!(psci_alive(ret2), CoreAlive::Running | CoreAlive::Other) {
                    crate::smp::ONLINE_MASK.set(slot);
                }
            }
            CoreAlive::Absent => {
                // Mark NOT online so boot won't try to wake it.
            }
        }
    }
}

fn record_descriptor(slot: usize, method: u8, enabled: bool, present: bool) {
    unsafe {
        let descs = &mut CPU_DESCRIPTORS;
        let count = CPU_DESCRIPTOR_COUNT;
        // If we've already recorded this slot, overwrite.
        for i in 0..count {
            if descs[i].reg as usize == slot {
                descs[i] = CpuDescStatic {
                    reg: slot as u32,
                    enable_method: method,
                    enabled,
                    present,
                };
                return;
            }
        }
        if count < descs.len() {
            descs[count] = CpuDescStatic {
                reg: slot as u32,
                enable_method: method,
                enabled,
                present,
            };
            CPU_DESCRIPTOR_COUNT = count + 1;
        }
    }
}

fn possible_to_u64() -> u64 {
    let mut bits = 0u64;
    for s in 0..MAX_CORES {
        if POSSIBLE_MASK.test(s) {
            bits |= 1u64 << s;
        }
        if PRESENT_MASK.test(s) {
            bits |= 1u64 << s;
        }
    }
    bits
}

fn log_summary() {
    let mut possible_count = 0usize;
    let mut present_count = 0usize;
    for s in 0..MAX_CORES {
        if POSSIBLE_MASK.test(s) {
            possible_count += 1;
        }
        if PRESENT_MASK.test(s) {
            present_count += 1;
        }
    }
    crate::log_info!(
        "SMP",
        "topology: possible={} present={} (online decided at boot)",
        possible_count, present_count
    );
}
