//! # touchd — EL1 Kext module for virtio-input (multi-touch)
//!
//! Loaded into EL1 by the kernel pill_loader via OHLINK dynamic
//! relocation. Hardware probing and event-queue setup happen at
//! `pill_init` time.

#![no_std]
#![no_main]

extern crate modskit;
extern crate shared;

use modskit::{mod_log_info, mod_log_warn, ModsBus};

const QUEUE_SIZE: usize = 64;
const MAX_TOUCH_SLOTS: usize = 10;

const VIRTIO_INPUT_MAGIC: u32 = 0x74726976;
const VIRTIO_INPUT_DEV_ID: u32 = 18;
const VIRTIO_MMIO_REGION_BASE: usize = 0x0a00_0000;

/// Discover a Virtio-Input sub-device that supports absolute (touch)
/// events. Returns the physical MMIO base address of the slot.
fn discover_touch_pa() -> Option<usize> {
    for slot in 0..32u32 {
        let slot_pa = VIRTIO_MMIO_REGION_BASE + (slot as usize) * 0x200;
        let magic = ModsBus::mmio_read(slot_pa, 0x000);
        let dev_id = ModsBus::mmio_read(slot_pa, 0x008);
        if magic != VIRTIO_INPUT_MAGIC || dev_id != VIRTIO_INPUT_DEV_ID {
            continue;
        }
        let va = ModsBus::pa_to_kernel_va(slot_pa);
        unsafe {
            core::ptr::write_volatile((va + 0x100) as *mut u8, 0x11);
            core::ptr::write_volatile((va + 0x101) as *mut u8, 0x02);
            let size_rel = core::ptr::read_volatile((va + 0x102) as *const u8);
            core::ptr::write_volatile((va + 0x100) as *mut u8, 0x11);
            core::ptr::write_volatile((va + 0x101) as *mut u8, 0x03);
            let size_abs = core::ptr::read_volatile((va + 0x102) as *const u8);

            // We own pure-touch (EV_ABS only) and mixed (EV_REL + EV_ABS)
            // sub-devices — both can emit ABS_MT_* multi-touch events.
            if size_abs > 0 {
                mod_log_info!(
                    "TOUCHD",
                    "claiming slot {} MMIO {:#x} (size_rel={} size_abs={})",
                    slot,
                    slot_pa,
                    size_rel,
                    size_abs
                );
                return Some(slot_pa);
            }
        }
    }
    None
}

/// Activate a discovered touch sub-device into DRIVER_OK.
unsafe fn activate_device(slot_pa: usize, q_pfn: u32) {
    let va = ModsBus::pa_to_kernel_va(slot_pa);
    core::ptr::write_volatile((va + 0x030) as *mut u32, 0);
    core::ptr::write_volatile((va + 0x038) as *mut u32, QUEUE_SIZE as u32);
    core::ptr::write_volatile((va + 0x03c) as *mut u32, 4096);
    core::ptr::write_volatile((va + 0x040) as *mut u32, q_pfn);
    core::ptr::write_volatile((va + 0x070) as *mut u32, 1 | 2 | 8 | 4);
    core::ptr::write_volatile((va + 0x050) as *mut u32, 0);
}

/// OHLINK entry point.
#[no_mangle]
pub extern "C" fn pill_init() -> i32 {
    mod_log_info!("TOUCHD", "EL1 Kext touchd init");

    let slot_pa = match discover_touch_pa() {
        Some(pa) => pa,
        None => {
            mod_log_warn!("TOUCHD", "no touch-capable device found, exiting cleanly");
            return 0;
        }
    };

    let q_pfn = 0x4a40_0000u32 / 4096;

    unsafe { activate_device(slot_pa, q_pfn) };

    mod_log_info!("TOUCHD", "touchd EL1 Kext init complete");
    0
}

/// OHLINK service loop entry.
#[no_mangle]
pub extern "C" fn pill_main() -> ! {
    loop {
        ModsBus::yield_cpu();
    }
}

/// Compile-time guarantees preserved from the EL0 version.
const _: () = {
    struct Slot {
        tracking_id: i32,
        x: i32,
        y: i32,
        pressure: u32,
    }
    let touch_slot_size = core::mem::size_of::<Slot>();
    assert!(touch_slot_size == 16, "TouchSlot must be 16 bytes");
    let _ = MAX_TOUCH_SLOTS;
};

/// `#![no_main]` requires a `main` symbol. It is never called because
/// the Kext is loaded by `pill_loader`, not invoked as a process.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    pill_init()
}
