#![no_std]
#![no_main]

extern crate hnxlibc;

#[no_mangle]
pub fn main() -> i32 {
    let msg = b"[EL0 testloader] Hello from EL0 userspace! Pangu is alive!\n";
    unsafe {
        hnxlibc::write(1, msg.as_ptr(), msg.len());
    }
    loop {}
}
