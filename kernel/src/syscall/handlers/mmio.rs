use shared::status::{Result, Status};

const RTC_BASE: usize = 0x09010000;

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
    } else {
        Err(Status::NotAllowed)
    }
}

pub fn sys_mmio_read(base: usize, offset: usize) -> Result<u32> {
    validate_mmio_base(base)?;
    let va = base.wrapping_add(offset);
    let val = unsafe { core::ptr::read_volatile(va as *const u32) };
    Ok(val)
}

pub fn sys_mmio_write(base: usize, offset: usize, value: u32) -> Result<()> {
    validate_mmio_base(base)?;
    let va = base.wrapping_add(offset);
    unsafe { core::ptr::write_volatile(va as *mut u32, value) };
    Ok(())
}
