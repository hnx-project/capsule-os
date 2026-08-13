//! # 🛸 PillsMod Dynamic Registration & Interface Contracts
//!
//! Defines the shared C-ABI interface between the CapsuleOS microkernel (EL1)
//! and loadable PillsMod (Kext) driver bundles.

use shared::status::Status;

/// Core Kernel Function Table imported by PillsMod.
/// Allows dynamic EL1 modules to access essential kernel services safely.
#[repr(C)]
pub struct KernelImportTable {
    /// Logging write callback into the kernel log buffer.
    pub log_write: extern "C" fn(tag: *const u8, tag_len: usize, msg: *const u8, msg_len: usize),
    /// Safe physical page allocation.
    pub alloc_pages: extern "C" fn(num_pages: usize) -> u64,
    /// Safe physical page freeing.
    pub free_pages: extern "C" fn(paddr: u64, num_pages: usize) -> i32,
}

/// The common trait that all dynamic PillsMod drivers can choose to implement.
pub trait PillsMod {
    /// Initialize the driver module.
    fn init(&self, kernel: &KernelImportTable) -> Result<(), Status>;
    /// Tear down and unload the driver module.
    fn exit(&self, kernel: &KernelImportTable) -> Result<(), Status>;
}
