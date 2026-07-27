use shared::device_info::*;
use shared::status::{Result, Status};

fn current_l0_pa() -> Option<usize> {
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let l0_pa = if let Some(t) = thread_ptr {
        let proc_id = unsafe { (*t).process_id };
        if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
            proc.page_table.l0_pa()
        } else {
            0
        }
    } else {
        0
    };
    if l0_pa == 0 { None } else { Some(l0_pa) }
}

pub fn sys_device_info(dst_user_va: usize, buf_len: usize) -> Result<usize> {
    if buf_len < 4 || dst_user_va == 0 {
        return Err(Status::InvalidArgs);
    }

    let boot = unsafe {
        if !crate::DEVICE_INFO_VALID {
            return Err(Status::NotFound);
        }
        &*crate::DEVICE_INFO_BOOT.as_ptr()
    };

    let max_records = (buf_len - 4) / DEVICE_RECORD_SIZE;
    let records_cap = max_records.min(MAX_DEVICE_RECORDS);

    let mut local = [0u8; DEVICE_BUFFER_SIZE];
    let mut idx: usize = 0;

    if boot.uart_base != 0 && idx < records_cap {
        let rec = DeviceInfoRecord::new(DEVICE_TYPE_UART, "pl011", boot.uart_base as u64, 4096, 33);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    if boot.gicd_base != 0 && idx < records_cap {
        let rec = DeviceInfoRecord::new(DEVICE_TYPE_GICD, "gicd", boot.gicd_base as u64, 4096, 0xFFFFFFFF);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    if boot.gicc_base != 0 && idx < records_cap {
        let rec = DeviceInfoRecord::new(DEVICE_TYPE_GICC, "gicc", boot.gicc_base as u64, 4096, 0xFFFFFFFF);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    if idx < records_cap {
        let rec = DeviceInfoRecord::new(DEVICE_TYPE_TIMER, "timer", 0, 0, 30);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    if idx < records_cap {
        let rec = DeviceInfoRecord::new(DEVICE_TYPE_RTC, "rtc", 0x09010000, 4096, 34);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    if idx < records_cap {
        let width = unsafe { crate::drivers::virtio_gpu::SCREEN_WIDTH };
        let height = unsafe { crate::drivers::virtio_gpu::SCREEN_HEIGHT };
        let rec = DeviceInfoRecord::new(DEVICE_TYPE_DISPLAY, "display", width as u64, height as u64, 0xFFFFFFFF);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    if idx < records_cap {
        // Poll latest events from physical mouse hardware
        crate::drivers::virtio_input::poll_events();

        let mouse_x = crate::drivers::virtio_input::get_mouse_x();
        let mouse_y = crate::drivers::virtio_input::get_mouse_y();
        let mouse_down = crate::drivers::virtio_input::is_mouse_down();

        // Compactly encode mouse state into base field
        let metadata = (mouse_x as u64 & 0xFFFF)
            | ((mouse_y as u64 & 0xFFFF) << 16)
            | ((if mouse_down { 1 } else { 0 }) << 32);

        let rec = DeviceInfoRecord::new(DEVICE_TYPE_MOUSE, "mouse", metadata, 0, 0xFFFFFFFF);
        write_record(&mut local, idx, &rec);
        idx += 1;
    }

    let total_bytes = 4 + idx * DEVICE_RECORD_SIZE;
    let count_bytes = (idx as u32).to_le_bytes();
    local[..4].copy_from_slice(&count_bytes);

    let copy_len = total_bytes.min(buf_len);
    let l0_pa = current_l0_pa().ok_or(Status::InvalidArgs)?;
    super::ipc::safe_copy_to_user(l0_pa, &local[..copy_len], dst_user_va, copy_len)?;

    Ok(copy_len)
}

fn write_record(buf: &mut [u8; DEVICE_BUFFER_SIZE], idx: usize, rec: &DeviceInfoRecord) {
    let offset = 4 + idx * DEVICE_RECORD_SIZE;
    let src = rec as *const DeviceInfoRecord as *const u8;
    unsafe {
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr().add(offset), DEVICE_RECORD_SIZE);
    }
}

/// SYSCALL_BLOCK_READ: read a single 512-byte sector from Virtio-Blk to user space.
pub fn sys_block_read(sector: u64, dst_user_va: usize) -> Result<()> {
    if dst_user_va == 0 {
        return Err(Status::InvalidArgs);
    }
    let l0_pa = current_l0_pa().ok_or(Status::InvalidArgs)?;
    let dst_pa = crate::arch::aarch64::mmu::translate_user_va(l0_pa, dst_user_va)
        .ok_or(Status::InvalidArgs)?;

    // Clean user space cache before read so we don't have stale/dirty lines
    unsafe {
        <crate::arch::CurrentArch as crate::arch::ArchHardware>::clean_and_invalidate_cache_range(dst_user_va, 512);
    }

    crate::drivers::virtio_blk::read_sector(sector, dst_pa)?;

    // Invalidate user space cache after read so CPU fetches the DMA'd data from RAM
    unsafe {
        <crate::arch::CurrentArch as crate::arch::ArchHardware>::clean_and_invalidate_cache_range(dst_user_va, 512);
    }

    Ok(())
}

/// SYSCALL_BLOCK_WRITE: write a single 512-byte sector from user space to Virtio-Blk.
pub fn sys_block_write(sector: u64, src_user_va: usize) -> Result<()> {
    if src_user_va == 0 {
        return Err(Status::InvalidArgs);
    }
    let l0_pa = current_l0_pa().ok_or(Status::InvalidArgs)?;
    let src_pa = crate::arch::aarch64::mmu::translate_user_va(l0_pa, src_user_va)
        .ok_or(Status::InvalidArgs)?;

    // Clean/writeback user space cache before write so physical RAM has the new data for DMA
    unsafe {
        <crate::arch::CurrentArch as crate::arch::ArchHardware>::clean_and_invalidate_cache_range(src_user_va, 512);
    }

    crate::drivers::virtio_blk::write_sector(sector, src_pa)
}

/// SYSCALL_BLOCK_SIZE: read the total number of sectors on the block device.
pub fn sys_block_size() -> Result<u64> {
    Ok(crate::drivers::virtio_blk::get_capacity())
}

/// SYSCALL_NET_SEND: send a raw network packet from user space through Virtio-Net.
pub fn sys_net_send(buf_user_va: usize, len: usize) -> Result<()> {
    if buf_user_va == 0 || len == 0 || len > 1518 {
        return Err(Status::InvalidArgs);
    }
    let l0_pa = current_l0_pa().ok_or(Status::InvalidArgs)?;
    
    let mut temp_buf = [0u8; 1518];
    super::ipc::safe_copy_from_user(l0_pa, buf_user_va, len, &mut temp_buf[..len])?;
    
    crate::drivers::virtio_net::send_packet(&temp_buf[..len])
}

/// SYSCALL_NET_RECV: receive a raw network packet from Virtio-Net into user space.
pub fn sys_net_recv(buf_user_va: usize, max_len: usize) -> Result<usize> {
    if buf_user_va == 0 || max_len == 0 {
        return Err(Status::InvalidArgs);
    }
    let l0_pa = current_l0_pa().ok_or(Status::InvalidArgs)?;
    
    let mut temp_buf = [0u8; 1518];
    let actual_len = crate::drivers::virtio_net::recv_packet(&mut temp_buf[..max_len.min(1518)])?;
    
    if actual_len > 0 {
        super::ipc::safe_copy_to_user(l0_pa, &temp_buf[..actual_len], buf_user_va, actual_len)?;
    }
    
    Ok(actual_len)
}

/// SYSCALL_DISPLAY_FLUSH: copy VMO window frame to Virtio-GPU backing memory and flush to screen.
pub fn sys_display_flush(table: &crate::object::handle_table::HandleTable, handle_raw: u32) -> Result<()> {
    let hv = shared::types::HandleValue::new(handle_raw);
    let rights = crate::object::rights::Rights::READ.bits();
    
    table.with_vmo(hv, rights, |vmo| {
        let size = vmo.size();
        let mut offset = 0;
        while offset < size {
            let page_idx = offset / 4096;
            let page_slot = vmo.page_slot(page_idx);
            if let Some(pa) = unsafe { *page_slot } {
                crate::drivers::virtio_gpu::copy_to_gpu_buffer(pa.as_usize(), offset);
            }
            offset += 4096;
        }
        
        crate::drivers::virtio_gpu::flush_to_screen();
    })
}
