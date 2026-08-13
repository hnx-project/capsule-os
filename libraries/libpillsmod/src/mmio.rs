//! # 🔌 Safe Volatile MMIO Access and CPU Barriers for AArch64
//!
//! Provides optimized, volatile register reads/writes and memory barriers
//! specifically for PillsMod driver execution at EL1.

/// Safe volatile write of a 32-bit register with a Data Synchronization Barrier (DSB).
#[inline(always)]
pub unsafe fn mmio_write32(addr: usize, value: u32) {
    core::ptr::write_volatile(addr as *mut u32, value);
    // Force complete memory write before continuing
    core::arch::asm!("dsb sy", options(nostack));
}

/// Safe volatile read of a 32-bit register with an Instruction Synchronization Barrier (ISB).
#[inline(always)]
pub unsafe fn mmio_read32(addr: usize) -> u32 {
    let val = core::ptr::read_volatile(addr as *const u32);
    core::arch::asm!("isb", options(nostack));
    val
}

/// Safe volatile write of an 8-bit register.
#[inline(always)]
pub unsafe fn mmio_write8(addr: usize, value: u8) {
    core::ptr::write_volatile(addr as *mut u8, value);
    core::arch::asm!("dsb sy", options(nostack));
}

/// Safe volatile read of an 8-bit register.
#[inline(always)]
pub unsafe fn mmio_read8(addr: usize) -> u8 {
    let val = core::ptr::read_volatile(addr as *const u8);
    core::arch::asm!("isb", options(nostack));
    val
}
