use core::arch::global_asm;

global_asm!(include_str!("boot_asm.S"));

const UART0_BASE: usize = 0x09000000;
const UART0_DR: *mut u8 = UART0_BASE as *mut u8;
const UART0_FR: *const u8 = (UART0_BASE + 0x18) as *const u8;

pub fn early_init() {
    let uart = UART0_BASE as *mut u32;
    unsafe {
        // Disable UART
        uart.add(0x30 / 4).write_volatile(0);
        // Set baud rate (integer)
        uart.add(0x24 / 4).write_volatile(13);
        // Set baud rate (fractional)
        uart.add(0x28 / 4).write_volatile(2);
        // Line control (8-bit, FIFO enabled)
        uart.add(0x2c / 4).write_volatile(0x70);
        // Control register (enable UART, RX, TX)
        uart.add(0x30 / 4).write_volatile(0x301);
    }
}

pub fn console_putchar(c: u8) {
    while unsafe { (UART0_FR.read_volatile() & (1 << 5)) != 0 } {}
    unsafe { UART0_DR.write_volatile(c); }
}

pub fn console_getchar() -> Option<u8> {
    while unsafe { (UART0_FR.read_volatile() & (1 << 4)) != 0 } {}
    let c = unsafe { UART0_DR.read_volatile() };
    if c == 0 { None } else { Some(c) }
}

pub mod boot {
    extern "C" {
        pub fn rust_boot();
    }
}
