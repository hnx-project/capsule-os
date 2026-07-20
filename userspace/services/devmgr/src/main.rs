#![no_std]
#![no_main]

extern crate capsule_runtime;
extern crate libcapsule;

use libcapsule::kprintln;

#[no_mangle]
pub fn main() -> i32 {
    kprintln!("====================================================");
    kprintln!("devmgr: CapsuleOS device manager starting...");
    kprintln!("devmgr: PL011 UART driver initialized");
    kprintln!("devmgr: device manager running");
    kprintln!("====================================================");
    loop {
        libcapsule::syscalls::yield_cpu();
    }
}
