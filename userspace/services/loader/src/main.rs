#![no_std]
#![no_main]

extern crate libcapsule;

use libcapsule::kprintln;

/// The standard boot-time Cap handle index where the BootFS VMO is injected by the kernel.
pub const BOOTFS_VMO_HANDLE: usize = 100;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("====================================================");
    kprintln!(
        "Loader: {} Active",
        option_env!("CAPSULEOS_VERSION").unwrap_or("capsuleOS Pangu v1.0.0 (dev)")
    );
    kprintln!("====================================================");

    // 1. Instantiate the stateless BootFS ServiceLoader
    let bootstrap_service = libcapsule::ServiceLoader::new(BOOTFS_VMO_HANDLE);

    // 2. Spawn 'initd' (Service Manager) from BootFS
    //    We deliberately use the user-mode `ServiceLoader` here, matching the
    //    same call shape that previously launched every L3 service.  This keeps
    //    the Loader as a thin EL0 bootstrap host without ever needing the
    //    kernel-level `sys_service_spawn` privileged path, so the BootFS VMO
    //    handle (slot 100) is never copied across process boundaries and
    //    initd resolves the VFS archive directly via the same primitive.
    kprintln!("Loader: Spawning 'initd' service manager via ServiceLoader...");
    match bootstrap_service.spawn_service("initd") {
        Ok(handle) => kprintln!("Loader: [SUCCESS] 'initd' spawned, handle={}", handle),
        Err(e) => kprintln!("Loader: [ERROR] Failed to spawn 'initd': {:?}", e),
    }

    kprintln!("Loader: Bootloader bootstrap hand-off to initd complete.");
    kprintln!("====================================================");

    // 3. Fallback yield loop to let other spawned services execute
    loop {
        libcapsule::syscalls::yield_cpu();
    }
}
