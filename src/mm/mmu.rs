//! Cross-architecture virtual memory helpers.
//!
//! On AArch64 we use a high-half kernel offset (the kernel lives in the
//! upper half of the 48-bit virtual address space). On RISC-V we keep an
//! identity mapping (VA == PA) for now.
//!
//! These helpers are `no_std` and safe to call from anywhere after
//! `phys::init` has run, because they only perform bit-twiddling.

use crate::fdt::BootInfo;

pub const PAGE_SIZE: usize = 4096;

/// AArch64 high-half offset for the kernel address space.
///
/// The offset is chosen so that the kernel's virtual address falls into a
/// 1 GiB L1 slot that does not collide with the identity-mapped low half
/// (L1[0]). With `0xFFFF_8000_0000_0000`, kernel PA 0x4008_0000 lands at
/// VA 0xFFFF_8000_4008_0000, whose L1 index is 0x100 (the 256th 1 GiB
/// region of the high half) and whose L0 index is 0x1FF.
#[cfg(target_arch = "aarch64")]
pub const KERNEL_OFFSET: usize = 0xFFFF_8000_0000_0000;

/// RISC-V: identity mapping (no offset).
#[cfg(target_arch = "riscv64")]
pub const KERNEL_OFFSET: usize = 0;

/// Convert a kernel virtual address back to its physical address.
///
/// On RISC-V this is a no-op because we identity-map.
#[inline(always)]
pub fn kernel_va_to_pa(va: usize) -> usize {
    va.wrapping_sub(KERNEL_OFFSET)
}

/// Convert a physical address into the kernel virtual address window.
#[inline(always)]
pub fn pa_to_kernel_va(pa: usize) -> usize {
    pa.wrapping_add(KERNEL_OFFSET)
}

/// Memory attribute classes used when populating page tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemAttr {
    /// Normal, write-back cacheable RAM.
    NormalCacheable,
    /// Device memory, non-cacheable, strongly ordered (nGnRnE on AArch64).
    Device,
}

/// A request to map a contiguous physical region.
#[derive(Debug, Clone, Copy)]
pub struct MappingRequest {
    pub pa: usize,
    pub size: usize,
    pub attr: MemAttr,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl MappingRequest {
    pub const fn new(pa: usize, size: usize, attr: MemAttr) -> Self {
        Self { pa, size, attr, readable: true, writable: true, executable: false }
    }
}

/// Architecture-specific MMU hook.
pub trait ArchMmu {
    /// Build the kernel page tables and enable the MMU.
    ///
    /// Called exactly once, after `mm::phys::init`. The UART base inside
    /// `boot` is the physical base parsed from the DTB.
    fn enable_with(boot: &BootInfo);
    /// Flush all TLB entries. Currently a no-op fallback.
    fn flush_tlb_all();
}
