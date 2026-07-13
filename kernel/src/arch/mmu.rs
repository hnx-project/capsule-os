#![allow(dead_code)]

//! Per-architecture MMU facade.
//!
//! The heavy lifting lives in `arch::aarch64::mmu` and `arch::riscv64::mmu`.
//! This module re-exports the architecture-specific types under a uniform
//! name and forwards the high-level `ArchMmu::enable_with` call.

pub use self::inner::*;

#[cfg(target_arch = "aarch64")]
mod inner {
    use shared::status::Result;
    pub use crate::arch::aarch64::mmu::{
        AArch64AddressSpace, AArch64Mmu, AArch64PageFlags, AArch64PageTable,
    };
    pub use crate::arch::aarch64::mmu::{MapFlags, map_page, unmap_page, map_page_under_l0};
    pub use crate::mm::mmu::pa_to_kernel_va;
    pub fn build_and_enable(ram_base: usize, ram_size: usize, uart_base: usize) -> Result<()> {
        crate::arch::aarch64::mmu::enable_inner(ram_base, ram_size, uart_base)
    }
}

#[cfg(target_arch = "riscv64")]
mod inner {
    use shared::status::Result;
    pub use crate::arch::riscv64::mmu::{
        RiscV64AddressSpace, RiscV64Mmu, RiscV64PageFlags, RiscV64PageTable,
    };
    pub use crate::arch::riscv64::mmu::{MapFlags, map_page, unmap_page};
    pub use crate::mm::mmu::pa_to_kernel_va;
    pub fn build_and_enable(ram_base: usize, ram_size: usize, uart_base: usize) -> Result<()> {
        crate::arch::riscv64::mmu::enable_inner(ram_base, ram_size, uart_base)
    }
}
