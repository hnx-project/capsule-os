#![no_std]

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BootInfo {
    pub magic: u32,
    pub version: u32,
    pub kernel_entry: usize,
    pub kernel_size: usize,
    pub kernel_load_addr: usize,
    pub dtb_addr: usize,
    pub memory_start: usize,
    pub memory_size: usize,
    pub cpu_count: u32,
    pub cmdline: [u8; 512],
}

impl BootInfo {
    pub const MAGIC: u32 = 0x43414D00;
    pub fn is_valid(&self) -> bool { self.magic == Self::MAGIC }
}
