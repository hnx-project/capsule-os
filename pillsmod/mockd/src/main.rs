#![no_std]
#![no_main]

extern crate modskit;
extern crate shared;

use modskit::{mod_log_info, ModsBus};

/// OHLINK entry point: invoked by the kernel pill_loader when this
/// `.pill` bundle is admitted into the system. Returns 0 on success.
#[no_mangle]
pub extern "C" fn pill_init() -> i32 {
    mod_log_info!("MOCKD", "EL1 Kext module loaded via OHLINK");

    // Probe a mock MMIO register (e.g. VirtIO MMIO slot 0 magic number).
    let magic = ModsBus::mmio_read(0x0a000000, 0x000);
    mod_log_info!("MOCKD", "MMIO read at 0x0a000000+0x000: magic={:#x}", magic);

    // Yield CPU to showcase scheduler interaction.
    for i in 1..=3 {
        mod_log_info!("MOCKD", "tick {}", i);
        ModsBus::yield_cpu();
    }

    mod_log_info!("MOCKD", "mockd EL1 Kext shut down cleanly");
    0
}

/// `#[no_main]` requires a `main` symbol. It is never called because
/// the Kext is loaded by `pill_loader`, not invoked as a process.
#[no_mangle]
pub extern "C" fn main() -> i32 {
    pill_init()
}