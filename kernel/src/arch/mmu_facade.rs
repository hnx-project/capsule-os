//! Per-architecture MMU facade.

pub use self::inner::*;

#[cfg(target_arch = "aarch64")]
mod inner {
    use shared::status::Result;
    pub use crate::arch::aarch64::mmu::{
        AArch64AddressSpace, AArch64Mmu, AArch64PageFlags, AArch64PageTable,
    };
    pub use crate::arch::aarch64::mmu::{MapFlags, map_page, unmap_page};
    pub use crate::arch::aarch64::mmu::{PAGE_SIZE, KERNEL_OFFSET, MemAttr, pa_to_kernel_va, ArchMmu};
    pub fn build_and_enable(ram_base: usize, ram_size: usize, uart_base: usize) -> Result<()> {
        crate::arch::aarch64::mmu::enable_inner(ram_base, ram_size, uart_base)
    }
}
