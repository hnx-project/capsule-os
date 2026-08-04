//! # modskit — EL1 Kernel Module SDK for CapsuleOS
//!
//! `modskit` is the **EL1 kernel-state** counterpart to `pillskit`. While
//! `pillskit` provides syscall-based access for **EL0 user-mode drivers**
//! (à la macOS DriverKit), `modskit` provides direct kernel-exported ABI
//! access for **EL1 kernel extensions** (à la macOS KEXT).
//!
//! Every function in this SDK ultimately resolves to an `extern "C"`
//! symbol exported by the HNX microkernel via its symbol table. The
//! kernel's `pill_loader` resolves these symbols at OHLINK load time
//! when the `.pill` bundle is admitted into the system.

#![no_std]

use shared::status::Result;

#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ModQueueHandles {
    pub desc_vmo: u64,
    pub avail_vmo: u64,
    pub used_vmo: u64,
    pub desc_bytes: u64,
    pub avail_bytes: u64,
    pub used_bytes: u64,
    pub qsize: u32,
}

/// Unified bus interface for EL1 kernel modules.
///
/// Every member of `ModsBus` resolves to a kernel-exported symbol; there
/// is no EL0 syscall fallback. This is the boundary that keeps the
/// microkernel's privilege discipline intact while still giving modules
/// high-performance access to hardware and IPC primitives.
pub struct ModsBus;

impl ModsBus {
    /// Translate a physical address into the kernel's mirror virtual address.
    pub fn pa_to_kernel_va(pa: usize) -> usize {
        unsafe { extern_kernel::pa_to_kernel_va(pa) }
    }

    /// Read a 32-bit register from device MMIO space (physical address).
    pub fn mmio_read(base_pa: usize, offset: usize) -> u32 {
        let va = unsafe { extern_kernel::pa_to_kernel_va(base_pa + offset) };
        unsafe { core::ptr::read_volatile(va as *const u32) }
    }

    /// Write a 32-bit register into device MMIO space (physical address).
    pub fn mmio_write(base_pa: usize, offset: usize, val: u32) -> Result<()> {
        let va = unsafe { extern_kernel::pa_to_kernel_va(base_pa + offset) };
        unsafe { core::ptr::write_volatile(va as *mut u32, val) };
        Ok(())
    }

    /// Set up a hardware virtqueue. Returns the queue handles for EL1 use.
    pub fn virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<ModQueueHandles> {
        unsafe { extern_kernel::kernel_virtio_setup_queue(slot, qsel, qsize) }
    }

    /// Kick (notify) the device that a virtqueue has new descriptors.
    pub fn virtio_kick(slot: u32, qsel: u16) -> Result<()> {
        unsafe { extern_kernel::kernel_virtio_kick(slot, qsel) }
    }

    /// Read the device's ISR (Interrupt Status Register).
    pub fn virtio_read_isr(slot: u32) -> Result<u32> {
        unsafe { extern_kernel::kernel_virtio_read_isr(slot) }
    }

    /// Map a VMO into the module's address space at `target_va`.
    pub fn map_memory(vmo: usize, target_va: usize, len: usize, perms: u32) -> Result<()> {
        unsafe { extern_kernel::kernel_vmar_map_self(vmo, target_va, len, perms) }
    }

    /// Flush display or cache lines backing a VMO into physical RAM.
    pub fn display_flush(vmo: usize) {
        unsafe { extern_kernel::kernel_display_flush(vmo) }
    }

    /// Yield the current thread's execution slice.
    pub fn yield_cpu() {
        unsafe { extern_kernel::kernel_thread_yield() }
    }

    /// Emit an INFO-level log line through the kernel log buffer.
    pub fn log_info(tag: &str, msg: &str) {
        unsafe { extern_kernel::kernel_log(tag.as_ptr(), tag.len(), msg.as_ptr(), msg.len()) }
    }

    /// Emit a WARN-level log line through the kernel log buffer.
    pub fn log_warn(tag: &str, msg: &str) {
        unsafe { extern_kernel::kernel_log(tag.as_ptr(), tag.len(), msg.as_ptr(), msg.len()) }
    }

    /// Emit an ERROR-level log line through the kernel log buffer.
    pub fn log_error(tag: &str, msg: &str) {
        unsafe { extern_kernel::kernel_log(tag.as_ptr(), tag.len(), msg.as_ptr(), msg.len()) }
    }
}

/// High-level unified logger macros for EL1 kernel modules.
///
/// These macros delegate to the kernel's log subsystem via
/// `ModsBus::log_*`. They are direct counterparts to the
/// `pill_log_*!` family in `pillskit`.
#[macro_export]
macro_rules! mod_log_info {
    ($tag:expr, $fmt:expr $(, $arg:expr)*) => {{
        let tag_str: &str = $tag;
        // Lightweight formatting into a stack buffer.
        let mut buf = [0u8; 200];
        let mut len = 0usize;
        {
            struct W<'a>(&'a mut [u8], &'a mut usize);
            impl<'a> core::fmt::Write for W<'a> {
                fn write_str(&mut self, s: &str) -> core::fmt::Result {
                    let bytes = s.as_bytes();
                    let take = core::cmp::min(bytes.len(), self.0.len() - *self.1);
                    self.0[*self.1..*self.1 + take].copy_from_slice(&bytes[..take]);
                    *self.1 += take;
                    Ok(())
                }
            }
            let mut w = W(&mut buf, &mut len);
            let _ = core::fmt::write(&mut w, format_args!($fmt $(, $arg)*));
        }
        $crate::ModsBus::log_info(tag_str, core::str::from_utf8(&buf[..len]).unwrap_or(""));
    }};
}

#[macro_export]
macro_rules! mod_log_warn {
    ($tag:expr, $fmt:expr $(, $arg:expr)*) => {{
        let tag_str: &str = $tag;
        let mut buf = [0u8; 200];
        let mut len = 0usize;
        {
            struct W<'a>(&'a mut [u8], &'a mut usize);
            impl<'a> core::fmt::Write for W<'a> {
                fn write_str(&mut self, s: &str) -> core::fmt::Result {
                    let bytes = s.as_bytes();
                    let take = core::cmp::min(bytes.len(), self.0.len() - *self.1);
                    self.0[*self.1..*self.1 + take].copy_from_slice(&bytes[..take]);
                    *self.1 += take;
                    Ok(())
                }
            }
            let mut w = W(&mut buf, &mut len);
            let _ = core::fmt::write(&mut w, format_args!($fmt $(, $arg)*));
        }
        $crate::ModsBus::log_warn(tag_str, core::str::from_utf8(&buf[..len]).unwrap_or(""));
    }};
}

#[macro_export]
macro_rules! mod_log_error {
    ($tag:expr, $fmt:expr $(, $arg:expr)*) => {{
        let tag_str: &str = $tag;
        let mut buf = [0u8; 200];
        let mut len = 0usize;
        {
            struct W<'a>(&'a mut [u8], &'a mut usize);
            impl<'a> core::fmt::Write for W<'a> {
                fn write_str(&mut self, s: &str) -> core::fmt::Result {
                    let bytes = s.as_bytes();
                    let take = core::cmp::min(bytes.len(), self.0.len() - *self.1);
                    self.0[*self.1..*self.1 + take].copy_from_slice(&bytes[..take]);
                    *self.1 += take;
                    Ok(())
                }
            }
            let mut w = W(&mut buf, &mut len);
            let _ = core::fmt::write(&mut w, format_args!($fmt $(, $arg)*));
        }
        $crate::ModsBus::log_error(tag_str, core::str::from_utf8(&buf[..len]).unwrap_or(""));
    }};
}

/// Kernel-exported symbols resolved by OHLINK relocation at load time.
mod extern_kernel {
    use shared::status::Result;

    extern "C" {
        pub fn pa_to_kernel_va(pa: usize) -> usize;
        pub fn kernel_virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<crate::ModQueueHandles>;
        pub fn kernel_virtio_kick(slot: u32, qsel: u16) -> Result<()>;
        pub fn kernel_virtio_read_isr(slot: u32) -> Result<u32>;
        pub fn kernel_vmar_map_self(vmo_handle: usize, target_va: usize, len: usize, perms: u32) -> Result<()>;
        pub fn kernel_display_flush(vmo: usize);
        pub fn kernel_thread_yield();
        pub fn kernel_log(tag_ptr: *const u8, tag_len: usize, msg_ptr: *const u8, msg_len: usize);
    }
}