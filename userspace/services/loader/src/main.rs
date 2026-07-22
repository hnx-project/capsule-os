#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::kprintln;

/// The standard boot-time Cap handle index where the BootFS VMO is injected by the kernel.
pub const BOOTFS_VMO_HANDLE: usize = 100;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("====================================================");
    kprintln!("Loader: {} Active", option_env!("CAPSULEOS_VERSION").unwrap_or("capsuleOS Pangu v1.0.0 (dev)"));
    kprintln!("====================================================");

    // 1. Spawn 'initd' (Service Manager) from BootFS with BootFS VMO handle hand-off
    kprintln!("Loader: Spawning 'initd' service manager via ServiceLauncher...");
    let desc = shared::launcher::ServiceDescriptor {
        name: "initd",
        path: "system/bin/initd",
        bootstrap_vmo_handle_index: Some(100),
    };
    match libcapsule::syscalls::service_spawn(&desc) {
        Ok(pid) => kprintln!("Loader: [SUCCESS] 'initd' spawned, PID={}", pid),
        Err(e) => kprintln!("Loader: [ERROR] Failed to spawn 'initd': {:?}", e),
    }

    kprintln!("Loader: Bootloader bootstrap hand-off to initd complete.");
    kprintln!("====================================================");

    // 4. Fallback yield loop to let other spawned services execute
    loop {
        libcapsule::syscalls::yield_cpu();
    }
}
