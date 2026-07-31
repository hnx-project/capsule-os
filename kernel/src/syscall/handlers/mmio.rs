use shared::status::{Result, Status};

const RTC_BASE: usize = 0x09010000;
const VIRTIO_MMIO_BASE: usize = 0x0a00_0000;
const VIRTIO_MMIO_END: usize = VIRTIO_MMIO_BASE + 32 * 0x200;

fn validate_mmio_base(base: usize) -> Result<()> {
    let boot = unsafe {
        if !crate::DEVICE_INFO_VALID {
            return Err(Status::NotFound);
        }
        &*crate::DEVICE_INFO_BOOT.as_ptr()
    };
    if base == boot.uart_base
        || base == boot.gicd_base
        || base == boot.gicc_base
        || base == RTC_BASE
    {
        Ok(())
    } else if base >= VIRTIO_MMIO_BASE && base < VIRTIO_MMIO_END {
        // Per the microkernel principle, EL0 device drivers own the
        // virtio protocol layer; they get raw MMIO access through
        // this syscall so they don't have to mmap the entire
        // 0x0a000000 region into their address space.
        Ok(())
    } else {
        Err(Status::NotAllowed)
    }
}

pub fn sys_mmio_read(base: usize, offset: usize) -> Result<u32> {
    validate_mmio_base(base)?;
    // All approved MMIO regions sit below 0x4000_0000; the kernel
    // identity-maps that range, but we still route through
    // pa_to_kernel_va so the access uses the kernel's high-half
    // mirror (TTBR1_EL1) — required because TTBR0_EL1 holds the
    // caller's user page table during a syscall.
    let va = unsafe { crate::arch::mmu_facade::pa_to_kernel_va(base.wrapping_add(offset)) };
    let val = unsafe { core::ptr::read_volatile(va as *const u32) };
    Ok(val)
}

pub fn sys_mmio_write(base: usize, offset: usize, value: u32) -> Result<()> {
    validate_mmio_base(base)?;
    let va = unsafe { crate::arch::mmu_facade::pa_to_kernel_va(base.wrapping_add(offset)) };
    unsafe { core::ptr::write_volatile(va as *mut u32, value) };
    Ok(())
}
