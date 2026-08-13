#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn pillsmod_init() -> i32 {
    12345
}

#[no_mangle]
pub extern "C" fn pillsmod_exit() -> i32 {
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
