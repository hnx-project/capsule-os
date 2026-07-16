#![no_std]
#![no_main]

extern crate libstd;

#[no_mangle]
pub fn main() -> i32 {
    libstd::io::print("devmgr: CapsuleOS device manager starting...\n");
    libstd::io::print("devmgr: PL011 UART driver initialized\n");
    libstd::io::print("devmgr: device manager running\n");
    loop {
        libstd::thread::yield_now();
    }
}
