// ----------------------------------------------------------------------------
// QEMU Virt 平台抽象与常量
// ----------------------------------------------------------------------------

pub static mut UART_BASE_ADDR: usize = 0x0900_0000;
pub const DTB_FALLBACK_ADDR: usize = 0x4200_0000;
pub const DEFAULT_OHC_BASE: usize = 0x4070_0000;
pub const DEFAULT_BOOTFS_BASE: usize = 0x4600_0000;

/// 平台特定的串口单字节输出
pub fn putchar(c: u8) {
    unsafe {
        // 硬件 MMIO 写入。对于 PL011 / NS16550 必须是 u32 写入对齐
        core::ptr::write_volatile(UART_BASE_ADDR as *mut u32, c as u32);
    }
}

/// 平台特定硬件初始化
pub fn init() {
    // 可以在这里做一些早期硬件初始化。由于 QEMU 已完成，可留空
}
