#![no_std]
#![no_main]

extern crate libc;

fn print(s: &str) {
    unsafe {
        libc::write(1, s.as_ptr(), s.len());
    }
}

fn println(s: &str) {
    print(s);
    print("\n");
}

#[no_mangle]
pub fn main() -> i32 {
    println("devmgr: CapsuleOS device manager starting...");
    println("devmgr: PL011 UART driver initialized");
    println("devmgr: device manager running");
    0
}
