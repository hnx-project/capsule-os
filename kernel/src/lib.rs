#![no_std]
#![crate_type = "staticlib"]

extern crate hal;
extern crate shared;

pub mod arch;
pub mod fdt;
pub mod drivers;
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
            if boot.uart_type.as_str() == "pl011" {
                drivers::uart::init_pl011(boot.uart_base);
            } else if boot.uart_type.as_str() == "ns16550" {
                drivers::uart::init_ns16550(boot.uart_base);
            }
            arch::early_init();

            print("\r\nCapsuleOS v0.2.0-dev\r\n");
            print("[FDT] Discovered hardware:\r\n");
            print(&format_boot_info(&boot));

            mm::init(boot.ram_base, boot.ram_size);
            print(&format_mem_info());
            print("OK\r\n");
        }
        Err(e) => {
            #[cfg(target_arch = "riscv64")]
            drivers::uart::init_ns16550(0x10000000);
            #[cfg(not(target_arch = "riscv64"))]
            drivers::uart::init_pl011(0x09000000);

            arch::early_init();
            print("\r\nCapsuleOS v0.2.0-dev\r\n");
            print("[FDT] FDT parse failed: ");
            print(e);
            print("\r\nUsing default architecture UART fallback\r\n");

            #[cfg(target_arch = "riscv64")]
            mm::init(0x80000000, 512 * 1024 * 1024);
            #[cfg(not(target_arch = "riscv64"))]
            mm::init(0x40000000, 512 * 1024 * 1024);

            print(&format_mem_info());
            print("OK\r\n");
        }
    }

    loop {}
}

fn format_mem_info() -> heapless::String<128> {
    use core::fmt::Write;
    let mut s: heapless::String<128> = heapless::String::new();
    let free_cnt = mm::phys::get_free_pages_count();
    let total_cnt = mm::phys::get_total_pages_count();
    let _ = write!(
        s,
        "[MM] Physical page allocator initialized.\r\n  Free pages : {} ({} MB) / {} ({} MB)\r\n",
        free_cnt,
        free_cnt * 4 / 1024,
        total_cnt,
        total_cnt * 4 / 1024
    );
    s
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