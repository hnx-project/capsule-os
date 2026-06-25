#![no_std]
#![crate_type = "staticlib"]

extern crate hal;
extern crate shared;

pub mod arch;
pub mod fdt;
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

    match fdt::parse(dtb_ptr) {
        Ok(boot) => {
            arch::set_uart_base(boot.uart_base);
            arch::early_init();

            print("\r\nCapsuleOS v0.2.0-dev\r\n");
            print("[FDT] Discovered hardware:\r\n");
            print(&format_boot_info(&boot));
            print("OK\r\n");
        }
        Err(e) => {
            arch::early_init();
            print("\r\nCapsuleOS v0.2.0-dev\r\n");
            print("[FDT] FDT parse failed: ");
            print(e);
            print("\r\nUsing default UART base 0x09000000\r\n");
            print("OK\r\n");
        }
    }

    loop {}
}

fn format_boot_info(boot: &fdt::BootInfo) -> heapless::String<128> {
    use core::fmt::Write;
    let mut s: heapless::String<128> = heapless::String::new();
    let _ = write!(s, "  UART base : {:#x}\r\n", boot.uart_base);
    let _ = write!(s, "  RAM base  : {:#x}\r\n", boot.ram_base);
    let _ = write!(s, "  RAM size  : {:#x} ({} MB)\r\n",
                   boot.ram_size,
                   boot.ram_size / 1024 / 1024);
    s
}

#[inline(never)]
fn print(s: &str) {
    for c in s.bytes() {
        arch::console_putchar(c);
    }
}