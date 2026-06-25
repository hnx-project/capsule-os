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

pub static mut DTB_POINTER: *const u8 = core::ptr::null();

#[no_mangle]
pub extern "C" fn kernel_main(dtb_ptr: *const u8) {
    unsafe {
        DTB_POINTER = dtb_ptr;
    }

    arch::early_init();

    for c in b"CapsuleOS v0.1.0\r\n" {
        arch::console_putchar(*c);
    }
    for c in b"OK\r\n" {
        arch::console_putchar(*c);
    }

    loop {}
}
