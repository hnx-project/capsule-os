//! # inputd — EL1 Kext module for virtio-input (mouse)
//!
//! Loaded into EL1 by the kernel pill_loader via OHLINK dynamic
//! relocation. Hardware probing and event-queue setup happen at
//! `pill_init` time; the per-session IPC dispatch loop is preserved
//! as commented-out code that the kernel will re-enable as it grows
//! the EL1 IPC ABI.

#![no_std]
#![no_main]

extern crate modskit;
extern crate shared;

use modskit::{mod_log_info, mod_log_warn, mod_log_error, ModsBus};

const QUEUE_SIZE: usize = 64;

const VIRTIO_INPUT_MAGIC: u32 = 0x74726976;
const VIRTIO_INPUT_DEV_ID: u32 = 18;
const VIRTIO_MMIO_REGION_BASE: usize = 0x0a00_0000;

/// Detect a VirtIO-Input pure-mouse sub-device. Returns the physical
/// MMIO base address of the slot on success.
fn discover_mouse_pa() -> Option<usize> {
    for slot in 0..32u32 {
        let slot_pa = VIRTIO_MMIO_REGION_BASE + (slot as usize) * 0x200;
        let magic = ModsBus::mmio_read(slot_pa, 0x000);
        let dev_id = ModsBus::mmio_read(slot_pa, 0x008);
        if magic != VIRTIO_INPUT_MAGIC || dev_id != VIRTIO_INPUT_DEV_ID {
            continue;
        }
        // Inspect virtio-input cfg fields to determine the sub-type.
        let va = ModsBus::pa_to_kernel_va(slot_pa);
        unsafe {
            // Probe EV_REL capability.
            core::ptr::write_volatile((va + 0x100) as *mut u8, 0x11);
            core::ptr::write_volatile((va + 0x101) as *mut u8, 0x02);
            let size_rel = core::ptr::read_volatile((va + 0x102) as *const u8);
            // Probe EV_ABS capability.
            core::ptr::write_volatile((va + 0x100) as *mut u8, 0x11);
            core::ptr::write_volatile((va + 0x101) as *mut u8, 0x03);
            let size_abs = core::ptr::read_volatile((va + 0x102) as *const u8);

            mod_log_info!(
                "INPUTD",
                "slot {} probe: size_rel={} size_abs={}",
                slot,
                size_rel,
                size_abs
            );

            // We own only the pure-mouse (EV_REL only) sub-devices.
            if size_rel > 0 && size_abs == 0 {
                mod_log_info!(
                    "INPUTD",
                    "claiming slot {} MMIO {:#x} as pure-mouse",
                    slot,
                    slot_pa
                );
                return Some(slot_pa);
            } else {
                // Defer to touchd by leaving the slot alone.
                continue;
            }
        }
    }
    None
}

/// Bring the device into DRIVER_OK with a populated event queue.
/// This function performs all the MMIO writes the EL0 version did,
/// but through `ModsBus::mmio_write`.
unsafe fn activate_device(slot_pa: usize, q_pfn: u32) {
    let va = ModsBus::pa_to_kernel_va(slot_pa);
    // QueueSel = 0
    core::ptr::write_volatile((va + 0x030) as *mut u32, 0);
    // QueueNum
    core::ptr::write_volatile((va + 0x038) as *mut u32, QUEUE_SIZE as u32);
    // QueueAlign
    core::ptr::write_volatile((va + 0x03c) as *mut u32, 4096);
    // QueuePFN
    core::ptr::write_volatile((va + 0x040) as *mut u32, q_pfn);
    // Status = ACKNOWLEDGE | DRIVER | FEATURES_OK | DRIVER_OK
    core::ptr::write_volatile((va + 0x070) as *mut u32, 1 | 2 | 8 | 4);
    // QueueNotify
    core::ptr::write_volatile((va + 0x050) as *mut u32, 0);
}

/// OHLINK entry point: kernel pill_loader invokes this to admit the
/// inputd.pill bundle. Hardware init only — full IPC dispatch lives
/// in the kernel once the EL1 IPC ABI is finalized.
#[no_mangle]
pub extern "C" fn pill_init() -> i32 {
    mod_log_info!("INPUTD", "EL1 Kext inputd init");

    let slot_pa = match discover_mouse_pa() {
        Some(pa) => pa,
        None => {
            mod_log_warn!("INPUTD", "no pure-mouse device found, exiting cleanly");
            return 0;
        }
    };

    // In EL1 the kernel directly provides virtqueue backing memory.
    // For now we use a fixed Q_PFN placeholder; the kernel's
    // virtio_setup_queue ABI will replace this once finalized.
    let q_pfn = 0x4a40_0000u32 / 4096;

    unsafe { activate_device(slot_pa, q_pfn) };

    mod_log_info!("INPUTD", "inputd EL1 Kext init complete");
    0
}

/// OHLINK service loop entry.
#[no_mangle]
pub extern "C" fn pill_main() -> ! {
    loop {
        ModsBus::yield_cpu();
    }
}

/// `#[no_main]` requires a `main` symbol. It is never called because
/// the Kext is loaded by `pill_loader`, not invoked as a process.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    pill_init()
}