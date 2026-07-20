#![no_std]
#![no_main]

extern crate capsule_runtime;
extern crate libcapsule;

use libcapsule::kprintln;

/// The standard boot-time Cap handle index where the BootFS VMO is injected by the kernel.
pub const BOOTFS_VMO_HANDLE: usize = 100;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("====================================================");
    kprintln!("Loader: {} Active", option_env!("CAPSULEOS_VERSION").unwrap_or("capsuleOS Pangu v1.0.0 (dev)"));
    kprintln!("====================================================");

    // 1. Instantiating the stateless BootFS ServiceLoader and ProgramLoader
    let bootstrap_service = libcapsule::ServiceLoader::new(BOOTFS_VMO_HANDLE);
    let bootstrap_program = libcapsule::ProgramLoader::new(BOOTFS_VMO_HANDLE);

    // 2. Spawn 'fileagent' (VFS Agent) from BootFS
    kprintln!("Loader: Spawning 'fileagent' service...");
    match bootstrap_service.spawn_service("fileagent") {
        Ok(handle) => kprintln!("Loader: [SUCCESS] 'fileagent' spawned, handle={}", handle),
        Err(e) => kprintln!("Loader: [ERROR] Failed to spawn 'fileagent': {:?}", e),
    }

    // 3. Spawn 'testall' (VFS Test Suite)
    kprintln!("Loader: Spawning 'testall'...");
    match bootstrap_program.spawn_program("testall") {
        Ok(handle) => kprintln!("Loader: [SUCCESS] 'testall' spawned, handle={}", handle),
        Err(e) => kprintln!("Loader: [ERROR] Failed to spawn 'testall': {:?}", e),
    }

    kprintln!("Loader: Userboot bootstrap complete.");
    kprintln!("====================================================");

    // 4. Fallback yield loop to let other spawned services execute
    loop {
        libcapsule::syscalls::yield_cpu();
    }
}
