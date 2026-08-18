//! # 🛸 PillsMod Dynamic Registration & Interface Contracts
//!
//! Defines the shared C-ABI interface between the CapsuleOS microkernel (EL1)
//! and loadable PillsMod (Kext) driver bundles.

use shared::status::Status;

/// C-ABI block device operations table.
/// Exposed by loadable block driver modules to the kernel during dynamic registration.
#[repr(C)]
pub struct BlockDeviceOps {
    /// Read sectors from disk. Returns 0 on success, negative error code on failure.
    pub read_sectors: extern "C" fn(sector: u64, dst_pa: usize) -> i32,
    /// Write sectors to disk. Returns 0 on success, negative error code on failure.
    pub write_sectors: extern "C" fn(sector: u64, src_pa: usize) -> i32,
    /// Get the total disk capacity in 512-byte sectors.
    pub get_capacity: extern "C" fn() -> u64,
}

/// C-ABI network device operations table.
/// Exposed by loadable network driver modules to the kernel during dynamic registration.
#[repr(C)]
pub struct NetDeviceOps {
    /// Send raw packet. Returns 0 on success, negative status on failure.
    pub send_packet: extern "C" fn(buf_pa: usize, len: usize) -> i32,
    /// Receive raw packet. Returns packet length on success, 0 if empty, negative status on failure.
    pub recv_packet: extern "C" fn(buf_pa: usize, max_len: usize) -> i32,
}

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
    /// Clean and invalidate caches for a virtual memory range (e.g. for DMA buffer consistency).
    pub clean_invalidate_cache: extern "C" fn(kva: usize, len: usize),
    /// Dynamically register a block device driver back into the kernel.
    pub register_block_device: extern "C" fn(ops: *const BlockDeviceOps) -> i32,
    /// Dynamically register a network device driver back into the kernel.
    pub register_net_device: extern "C" fn(ops: *const NetDeviceOps) -> i32,
}

/// The common trait that all dynamic PillsMod drivers can choose to implement.
pub trait PillsMod {
    /// Initialize the driver module.
    fn init(&self, kernel: &KernelImportTable) -> Result<(), Status>;
    /// Tear down and unload the driver module.
    fn exit(&self, kernel: &KernelImportTable) -> Result<(), Status>;
}
