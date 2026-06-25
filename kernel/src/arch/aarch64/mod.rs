use core::arch::global_asm;

global_asm!(include_str!("boot_asm.S"));

static mut UART_BASE: usize = 0x09000000;

#[inline(always)]
fn uart_dr() -> *mut u8 {
    unsafe { UART_BASE as *mut u8 }
}

#[inline(always)]
fn uart_fr() -> *const u8 {
    unsafe { (UART_BASE + 0x18) as *const u8 }
}

#[inline(always)]
fn uart_reg(offset: usize) -> *mut u32 {
    unsafe { (UART_BASE + offset) as *mut u32 }
}

pub fn set_uart_base(base: usize) {
    unsafe { UART_BASE = base; }
}

pub fn get_uart_base() -> usize {
    unsafe { UART_BASE }
}

pub fn early_init() {
    unsafe {
        // 完全重置 UART
        uart_reg(0x30).write_volatile(0);          // Disable
        uart_reg(0x30).write_volatile(0x301);      // Enable, RX, TX
        uart_reg(0x24).write_volatile(13);         // 波特率
        uart_reg(0x28).write_volatile(2);          // 波特率小数
        uart_reg(0x2c).write_volatile(0x70);       // 8-bit, FIFO
        uart_reg(0x2c).write_volatile(0x71);       // 8-bit, FIFO, enable
        uart_reg(0x30).write_volatile(0x301);      // Re-enable
    }
}

pub fn console_putchar(c: u8) {
    while unsafe { (uart_fr().read_volatile() & (1 << 5)) != 0 } {}
    unsafe { uart_dr().write_volatile(c); }
}

pub fn console_getchar() -> Option<u8> {
    while unsafe { (uart_fr().read_volatile() & (1 << 4)) != 0 } {}
    let c = unsafe { uart_dr().read_volatile() };
    if c == 0 { None } else { Some(c) }
}

pub mod boot {
    extern "C" {
        pub fn rust_boot();
    }
}