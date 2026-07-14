// ----------------------------------------------------------------------------
// QEMU Virt 平台抽象与常量
// ----------------------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
pub const OHC_BASE: usize = 0x4070_0000;
#[cfg(target_arch = "aarch64")]
const UART_BASE: *mut u32 = 0x0900_0000 as *mut u32; // PL011 串口
#[cfg(target_arch = "aarch64")]
pub const DTB_FALLBACK_ADDR: usize = 0x4200_0000; // 由 QEMU -device loader 显式加载的 DTB 地址
#[cfg(target_arch = "aarch64")]
pub const BOOTFS_BASE: usize = 0x4600_0000;

#[cfg(target_arch = "riscv64")]
pub const OHC_BASE: usize = 0x8070_0000;
#[cfg(target_arch = "riscv64")]
const UART_BASE: *mut u32 = 0x1000_0000 as *mut u32; // NS16550 串口
#[cfg(target_arch = "riscv64")]
pub const DTB_FALLBACK_ADDR: usize = 0x8200_0000;
#[cfg(target_arch = "riscv64")]
pub const BOOTFS_BASE: usize = 0x8600_0000;

/// 平台特定的串口单字节输出
pub fn putchar(c: u8) {
    unsafe {
        // 硬件 MMIO 写入。对于 PL011 / NS16550 必须是 u32 写入对齐
        core::ptr::write_volatile(UART_BASE, c as u32);
    }
}

/// 平台特定硬件初始化
pub fn init() {
    // 可以在这里做一些早期硬件初始化。由于 QEMU 已完成，可留空
}
