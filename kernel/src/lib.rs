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

#[no_mangle]
pub extern "C" fn kernel_main() {
    // Print "OK\r\n" using inline assembly
    unsafe {
        core::arch::asm!(
            r"
            movz x0, #0x0000
            movk x0, #0x0900, lsl #16
            movz w1, #0x4F
            strb w1, [x0]
            movz w1, #0x4B
            strb w1, [x0]
            movz w1, #0x0D
            strb w1, [x0]
            movz w1, #0x0A
            strb w1, [x0]
            ",
            options(nostack)
        );
    }
    loop {}
}
