#![no_std]
#![no_main]

extern crate hnxlibc;

#[no_mangle]
extern "C" fn _start() {
    let msg = "Hello from CapsuleOS EL0 Userspace Loader Service!\n";
    let _ = hnxlibc::write(1, msg.as_ptr(), msg.len());
    loop {}
}
