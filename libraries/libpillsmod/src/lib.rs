//! # 🛸 libpillsmod: Dynamic EL1 Driver Support Library
//!
//! Provides the core runtime, panic handler, safe volatile MMIO access,
//! and standard C-ABI interface bindings for loadable dynamic kernel modules
//! (PillsMod) in CapsuleOS.

#![no_std]

pub mod interfaces;
pub mod panic;
pub mod log;
pub mod mmio;

use core::sync::atomic::Ordering;
pub use interfaces::{KernelImportTable, PillsMod, BlockDeviceOps, NetDeviceOps};
pub use log::pill_print;

/// Initializes the dynamic PillsMod library runtime using the kernel handoff import table.
/// This connects the panic handler and log macros to the live kernel console.
pub fn init_pillsmod_runtime(kernel: &KernelImportTable) {
    panic::KERNEL_LOG_FN.store(kernel.log_write as usize, Ordering::SeqCst);
}
