#![no_std]
#![no_main]

use libpillsmod::{pill_print, KernelImportTable};

#[no_mangle]
#[link_section = ".entry"]
pub extern "C" fn pillsmod_init(kernel: &KernelImportTable) -> i32 {
    pill_print(kernel, "Hello from PillsMod! We are successfully initialized inside EL1 using libpillsmod direct print!");
    12345
}

#[no_mangle]
pub extern "C" fn pillsmod_exit(_kernel: &KernelImportTable) -> i32 {
    0
}
