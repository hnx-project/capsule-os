#![no_std]

use core::fmt;

pub struct BootInfo {
    pub uart_base: usize,
    pub ram_base: usize,
    pub ram_size: usize,
}

impl BootInfo {
    pub const fn empty() -> Self {
        BootInfo {
            uart_base: 0,
            ram_base: 0,
            ram_size: 0,
        }
    }
}

impl fmt::Display for BootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  UART base : {:#x}", self.uart_base)?;
        writeln!(f, "  RAM base  : {:#x}", self.ram_base)?;
        writeln!(f, "  RAM size  : {:#x} ({} MB)", self.ram_size, self.ram_size / 1024 / 1024)
    }
}

const FDT_MAGIC: u32 = 0xD00DFEED;
const DEFAULT_RAM_BASE: usize = 0x40000000;
const DEFAULT_RAM_SIZE: usize = 512 * 1024 * 1024; // 512 MB
const DEFAULT_UART_BASE: usize = 0x09000000;

/// 从设备树二进制解析硬件信息
///
/// 当前实现是简化的版本：
/// - 验证 FDT 魔数
/// - UART 默认地址 (QEMU virt)
/// - RAM 默认范围 (QEMU virt 512MB)
/// 后续可以从 FDT 中真实读取 /memory 和 /chosen 节点
pub fn parse(dtb_ptr: *const u8) -> Result<BootInfo, &'static str> {
    if dtb_ptr.is_null() {
        return Err("DTB pointer is null");
    }

    let magic = unsafe { core::ptr::read_volatile(dtb_ptr as *const u32) };
    if magic.to_be() != FDT_MAGIC {
        return Err("Invalid FDT magic");
    }

    // 当前使用默认值。后续实现完整的 FDT 解析。
    Ok(BootInfo {
        uart_base: DEFAULT_UART_BASE,
        ram_base: DEFAULT_RAM_BASE,
        ram_size: DEFAULT_RAM_SIZE,
    })
}