#![no_std]

#[cfg(target_env = "capsule")]
pub use capsule;

use shared::status::Result;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PillQueueHandles {
    pub desc_vmo: u64,
    pub avail_vmo: u64,
    pub used_vmo: u64,
    pub desc_bytes: u64,
    pub avail_bytes: u64,
    pub used_bytes: u64,
    pub qsize: u32,
}

/// Dynamic dynamic interface to talk to hardware bus.
pub struct PillsBus;

impl PillsBus {
    /// Read 32-bit register from device MMIO space
    pub fn mmio_read(base_pa: usize, offset: usize) -> u32 {
        #[cfg(target_env = "capsule")]
        {
            // EL0 User Mode (pilladdon): use system call via libcapsule
            capsule::syscalls::mmio_read(base_pa + offset, 0).unwrap_or(0)
        }
        #[cfg(not(target_env = "capsule"))]
        {
            // EL1 Kernel Mode (pillmod): direct read from kernel virtual address mirror
            unsafe {
                let va = extern_kernel::pa_to_kernel_va(base_pa + offset);
                core::ptr::read_volatile(va as *const u32)
            }
        }
    }

    /// Write 32-bit register to device MMIO space
    pub fn mmio_write(base_pa: usize, offset: usize, val: u32) -> Result<()> {
        #[cfg(target_env = "capsule")]
        {
            // EL0 User Mode (pilladdon): use system call via libcapsule
            capsule::syscalls::mmio_write(base_pa + offset, 0, val)
        }
        #[cfg(not(target_env = "capsule"))]
        {
            // EL1 Kernel Mode (pillmod): direct volatile write
            unsafe {
                let va = extern_kernel::pa_to_kernel_va(base_pa + offset);
                core::ptr::write_volatile(va as *mut u32, val);
            }
            Ok(())
        }
    }

    /// Setup a hardware virtqueue
    pub fn virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<PillQueueHandles> {
        #[cfg(target_env = "capsule")]
        {
            // EL0 User Mode (pilladdon): call kernel setup
            let handles = capsule::syscalls::virtio_setup_queue(slot, qsel, qsize)?;
            Ok(PillQueueHandles {
                desc_vmo: handles.desc_vmo,
                avail_vmo: handles.avail_vmo,
                used_vmo: handles.used_vmo,
                desc_bytes: handles.desc_bytes,
                avail_bytes: handles.avail_bytes,
                used_bytes: handles.used_bytes,
                qsize: handles.qsize,
            })
        }
        #[cfg(not(target_env = "capsule"))]
        {
            // EL1 Kernel Mode (pillmod): call kernel-exported direct setup function
            unsafe {
                let handles = extern_kernel::kernel_virtio_setup_queue(slot, qsel, qsize)?;
                Ok(PillQueueHandles {
                    desc_vmo: handles.desc_vmo,
                    avail_vmo: handles.avail_vmo,
                    used_vmo: handles.used_vmo,
                    desc_bytes: handles.desc_bytes,
                    avail_bytes: handles.avail_bytes,
                    used_bytes: handles.used_bytes,
                    qsize: handles.qsize,
                })
            }
        }
    }

    /// Kick the device queue
    pub fn virtio_kick(slot: u32, qsel: u16) -> Result<()> {
        #[cfg(target_env = "capsule")]
        {
            capsule::syscalls::virtio_kick(slot, qsel)
        }
        #[cfg(not(target_env = "capsule"))]
        {
            unsafe { extern_kernel::kernel_virtio_kick(slot, qsel) }
        }
    }

    /// Read ISR register
    pub fn virtio_read_isr(slot: u32) -> Result<u32> {
        #[cfg(target_env = "capsule")]
        {
            capsule::syscalls::virtio_read_isr(slot)
        }
        #[cfg(not(target_env = "capsule"))]
        {
            unsafe { extern_kernel::kernel_virtio_read_isr(slot) }
        }
    }

    /// Map memory to local address space
    pub fn map_memory(vmo: usize, target_va: usize, len: usize) -> Result<usize> {
        #[cfg(target_env = "capsule")]
        {
            capsule::syscalls::vmar_map_self(vmo, target_va, len, 11)
        }
        #[cfg(not(target_env = "capsule"))]
        {
            // In EL1, kernel-mode mapping
            unsafe {
                extern_kernel::kernel_vmar_map_self(vmo, target_va, len)?;
            }
            Ok(target_va)
        }
    }

    /// Flush display or caches
    pub fn display_flush(vmo: usize) {
        #[cfg(target_env = "capsule")]
        {
            let _ = capsule::syscalls::display_flush(vmo);
        }
        #[cfg(not(target_env = "capsule"))]
        {
            unsafe { extern_kernel::kernel_display_flush(vmo); }
        }
    }

    /// Yield execution slice
    pub fn yield_cpu() {
        #[cfg(target_env = "capsule")]
        {
            capsule::syscalls::yield_cpu();
        }
        #[cfg(not(target_env = "capsule"))]
        {
            unsafe { extern_kernel::kernel_thread_yield(); }
        }
    }
}

/// High-level Unified Logger macros for Pill drivers
#[macro_export]
macro_rules! pill_log_info {
    ($tag:expr, $fmt:expr $(, $arg:expr)*) => {{
        #[cfg(target_env = "capsule")]
        {
            $crate::capsule::log_info!($tag, $fmt $(, $arg)*);
        }
        #[cfg(not(target_env = "capsule"))]
        {
            $crate::print_kernel_log($tag, $fmt);
        }
    }};
}

#[macro_export]
macro_rules! pill_log_error {
    ($tag:expr, $fmt:expr $(, $arg:expr)*) => {{
        #[cfg(target_env = "capsule")]
        {
            $crate::capsule::log_error!($tag, $fmt $(, $arg)*);
        }
        #[cfg(not(target_env = "capsule"))]
        {
            $crate::print_kernel_log($tag, $fmt);
        }
    }};
}

#[cfg(not(target_env = "capsule"))]
pub fn print_kernel_log(tag: &str, msg: &str) {
    unsafe {
        extern_kernel::kernel_log(tag.as_ptr(), tag.len(), msg.as_ptr(), msg.len());
    }
}

/// Interface declarations for the symbols exported by the EL1 kernel
#[cfg(not(target_env = "capsule"))]
mod extern_kernel {
    use shared::status::Result;

    extern "C" {
        pub fn pa_to_kernel_va(pa: usize) -> usize;
        pub fn kernel_virtio_setup_queue(slot: u32, qsel: u16, qsize: u16) -> Result<crate::PillQueueHandles>;
        pub fn kernel_virtio_kick(slot: u32, qsel: u16) -> Result<()>;
        pub fn kernel_virtio_read_isr(slot: u32) -> Result<u32>;
        pub fn kernel_vmar_map_self(vmo_handle: usize, target_va: usize, len: usize) -> Result<()>;
        pub fn kernel_display_flush(vmo: usize);
        pub fn kernel_thread_yield();
        pub fn kernel_log(tag_ptr: *const u8, tag_len: usize, msg_ptr: *const u8, msg_len: usize);
    }
}
