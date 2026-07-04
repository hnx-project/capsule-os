#![no_std]
#![no_main]

extern crate hnxlibc;

fn print(s: &str) {
    unsafe {
        hnxlibc::write(1, s.as_ptr(), s.len());
    }
}

fn println(s: &str) {
    print(s);
    print("\n");
}

#[no_mangle]
extern "C" fn _start() {
    println("devmgr: CapsuleOS device manager starting...");
    println("devmgr: PL011 UART driver initialized");
    println("devmgr: device manager running");
    loop {}
}
