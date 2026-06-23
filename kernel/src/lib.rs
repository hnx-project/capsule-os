#![no_std]
#![crate_type = "staticlib"]

extern crate hal;
extern crate shared;

pub mod arch;
pub mod task;
pub mod mm;
pub mod ipc;
pub mod object;
pub mod syscall;
pub mod sync;
pub mod kcore;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

fn print_str(s: &str) {
    for byte in s.bytes() {
        arch::console_putchar(byte);
    }
}

fn print_hex(n: usize) {
    let hex = b"0123456789abcdef";
    for i in (0..16).rev() {
        arch::console_putchar(hex[(n >> (i * 4)) & 0xf]);
    }
}

#[no_mangle]
pub extern "C" fn _start() {
    print_str("CapsuleOS booting...\r\n");
    print_str("Hello from kernel!\r\n");
    loop {}
}
