#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::{log_info, log_error};

/// The standard boot-time Cap handle index where the Services VMO is injected by the kernel.
pub const SERVICES_VMO_HANDLE: usize = 101;

#[no_mangle]
pub fn main() -> i32 {
    log_info!(
        "LOADER",
        "Loader: {} Active",
        option_env!("CAPSULEOS_VERSION").unwrap_or("capsuleOS Pangu v1.0.0 (dev)")
    );

    // 1. Instantiate the stateless BootFS ServiceLoader on Services VMO
    let bootstrap_service = libcapsule::ServiceLoader::new(SERVICES_VMO_HANDLE);

    // 2. Spawn 'servicesd' (Service Manager) from BootFS
    //    We deliberately use the user-mode `ServiceLoader` here, matching the
    //    same call shape that previously launched every L3 service.  This keeps
    //    the Loader as a thin EL0 bootstrap host without ever needing the
    //    kernel-level `sys_service_spawn` privileged path, so the BootFS VMO
    //    handle (slot 100) is never copied across process boundaries and
    //    servicesd resolves the VFS archive directly via the same primitive.
    log_info!("LOADER", "Spawning 'servicesd' service manager via ServiceLoader...");
    match bootstrap_service.spawn_service("servicesd") {
        Ok(handle) => {
            log_info!("LOADER", "[SUCCESS] 'servicesd' spawned, handle={}", handle);
        }
        Err(e) => {
            log_error!("LOADER", "[ERROR] Failed to spawn 'servicesd': {:?}", e);
        }
    }

    log_info!("LOADER", "Bootloader bootstrap hand-off to servicesd complete.");

    // 3. Fallback yield loop to let other spawned services execute
    loop {
        libcapsule::syscalls::yield_cpu();
    }
}
