//! # graphicd — EL1 Kext module for virtio-gpu
//!
//! Loaded into EL1 by the kernel pill_loader via OHLINK dynamic
//! relocation. Performs hardware bring-up and virtqueue setup at
//! `pill_init` time.

#![no_std]
#![no_main]

extern crate modskit;
extern crate shared;

use modskit::{mod_log_info, mod_log_warn, mod_log_error, ModsBus};
use shared::status::{Result, Status};

const QUEUE_SIZE: usize = 16;
const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 960;

const VIRTIO_GPU_MAGIC: u32 = 0x74726976;
const VIRTIO_GPU_DEV_ID: u32 = 16;
const VIRTIO_MMIO_REGION_BASE: usize = 0x0a00_0000;

/// Discover a Virtio-GPU sub-device. Returns the physical MMIO base.
fn discover_gpu_pa() -> Option<usize> {
    for slot in 0..32u32 {
        let slot_pa = VIRTIO_MMIO_REGION_BASE + (slot as usize) * 0x200;
        let magic = ModsBus::mmio_read(slot_pa, 0x000);
        let dev_id = ModsBus::mmio_read(slot_pa, 0x008);
        if magic == VIRTIO_GPU_MAGIC && dev_id == VIRTIO_GPU_DEV_ID {
            mod_log_info!(
                "GPUD",
                "discovered Virtio-GPU at slot {} MMIO {:#x}",
                slot,
                slot_pa
            );
            return Some(slot_pa);
        }
    }
    None
}

/// Bring the device into DRIVER_OK with the standard virtio-gpu
/// negotiation sequence. Returns `true` on success.
unsafe fn activate_device(slot_pa: usize, q_pfn: u32) -> Result<()> {
    ModsBus::mmio_write(slot_pa, 0x070, 0)?;           // Reset
    ModsBus::mmio_write(slot_pa, 0x070, 1 | 2)?;       // ACKNOWLEDGE | DRIVER
    ModsBus::mmio_write(slot_pa, 0x014, 0)?;           // Device status
    let f0 = ModsBus::mmio_read(slot_pa, 0x010);
    ModsBus::mmio_write(slot_pa, 0x020, f0)?;          // Feature select
    ModsBus::mmio_write(slot_pa, 0x070, 1 | 2 | 8)?;   // FEATURES_OK
    ModsBus::mmio_write(slot_pa, 0x028, 4096)?;        // Guest page size
    ModsBus::mmio_write(slot_pa, 0x030, 0)?;           // QueueSel
    ModsBus::mmio_write(slot_pa, 0x038, QUEUE_SIZE as u32)?;
    ModsBus::mmio_write(slot_pa, 0x03c, 4096)?;        // QueueAlign
    ModsBus::mmio_write(slot_pa, 0x040, q_pfn)?;
    ModsBus::mmio_write(slot_pa, 0x070, 1 | 2 | 8 | 4)?; // DRIVER_OK
    Ok(())
}

/// OHLINK entry point. Brings up the GPU; the full framebuffer/flush
/// pipeline is gated on the kernel growing the virtio-gpu resource
/// commands ABI.
#[no_mangle]
pub extern "C" fn pill_init() -> i32 {
    mod_log_info!("GPUD", "EL1 Kext graphicd init");

    let slot_pa = match discover_gpu_pa() {
        Some(pa) => pa,
        None => {
            mod_log_warn!("GPUD", "no Virtio-GPU device found, exiting cleanly");
            return 0;
        }
    };

    let q_pfn = 0x4a4b0000u32 / 4096;

    if let Err(e) = unsafe { activate_device(slot_pa, q_pfn) } {
        mod_log_error!("GPUD", "device activation failed: {:?}", e);
        return -1;
    }

    mod_log_info!(
        "GPUD",
        "graphicd EL1 Kext init complete ({}x{} framebuffer target)",
        SCREEN_WIDTH,
        SCREEN_HEIGHT
    );
    0
}

/// OHLINK service loop entry.
#[no_mangle]
pub extern "C" fn pill_main() -> ! {
    loop {
        ModsBus::yield_cpu();
    }
}
/// `#![no_main]` requires a `main` symbol. It is never called because
/// the Kext is loaded by `pill_loader`, not invoked as a process.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    pill_init()
}
