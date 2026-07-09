use shared::status::{Result, Status};
use shared::types::HandleValue;
use crate::ipc::channel::Channel;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;

/// Helper function to safely copy data from user virtual space to kernel buffer
fn safe_copy_from_user(l0_pa: usize, src_user_va: usize, len: usize, dest: &mut [u8]) -> Result<()> {
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let mut copied = 0;
    while copied < len {
        let va = src_user_va + copied;
        let pa = crate::arch::translate_user_va(l0_pa, va).ok_or(Status::InvalidArgs)?;
        let kernel_va = crate::mm::mmu::pa_to_kernel_va(pa);
        let page_offset = va & 0xfff;
        let chunk_len = core::cmp::min(len - copied, 4096 - page_offset);
        unsafe {
            core::ptr::copy_nonoverlapping(
                (kernel_va + page_offset) as *const u8,
                dest.as_mut_ptr().add(copied),
                chunk_len
            );
        }
        copied += chunk_len;
    }
    Ok(())
}

/// Helper function to safely copy data from kernel buffer to user virtual space
fn safe_copy_to_user(l0_pa: usize, src: &[u8], dest_user_va: usize, len: usize) -> Result<()> {
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let mut copied = 0;
    while copied < len {
        let va = dest_user_va + copied;
        let pa = crate::arch::translate_user_va(l0_pa, va).ok_or(Status::InvalidArgs)?;
        let kernel_va = crate::mm::mmu::pa_to_kernel_va(pa);
        let page_offset = va & 0xfff;
        let chunk_len = core::cmp::min(len - copied, 4096 - page_offset);
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(copied),
                (kernel_va + page_offset) as *mut u8,
                chunk_len
            );
        }
        copied += chunk_len;
    }
    Ok(())
}

pub fn sys_channel_create(table: &HandleTable) -> Result<(HandleValue, HandleValue)> {
    let chan0 = Channel::new()?;
    let chan1 = Channel::new()?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();

    let h0 = table.add(KernelObject::Channel(chan0), rights)?;
    let h1 = table.add(KernelObject::Channel(chan1), rights)?;

    // Retrieve raw pointers of both Channel endpoints inside the HandleTable to link them as peers
    let c0_ptr = table.with_channel(h0, rights, |c0| c0 as *mut Channel)?;
    let c1_ptr = table.with_channel(h1, rights, |c1| c1 as *mut Channel)?;

    // Link peers together
    table.with_channel(h0, rights, |c0| {
        c0.peer = Some(c1_ptr);
    })?;
    table.with_channel(h1, rights, |c1| {
        c1.peer = Some(c0_ptr);
    })?;

    Ok((h0, h1))
}

pub fn sys_channel_read(table: &HandleTable, handle_raw: u32,
                        buf_ptr: usize, buf_len: usize) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    
    // Extract current thread's process page table base (L0_PA)
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let l0_pa = if let Some(t) = thread_ptr {
        let proc_id = unsafe { (*t).process_id };
        if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
            proc.l0_user_pa
        } else {
            0
        }
    } else {
        0
    };

    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    // Allocate a temporary kernel-level buffer to receive data from channel safely
    let mut temp_buf = heapless::Vec::<u8, 256>::new();
    let actual_len = core::cmp::min(buf_len, 256);
    unsafe {
        temp_buf.set_len(actual_len);
    }

    // Call inner channel read
    let read_bytes = table.with_channel(hv, Rights::READ.bits(), |chan| {
        chan.read(&mut temp_buf[..actual_len], &mut [])
    })??;

    // Safely write results back to user virtual space
    safe_copy_to_user(l0_pa, &temp_buf[..read_bytes], buf_ptr, read_bytes)?;

    Ok(read_bytes)
}

pub fn sys_channel_write(table: &HandleTable, handle_raw: u32,
                         buf_ptr: usize, buf_len: usize) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);

    // Extract current thread's process page table base (L0_PA)
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let l0_pa = if let Some(t) = thread_ptr {
        let proc_id = unsafe { (*t).process_id };
        if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
            proc.l0_user_pa
        } else {
            0
        }
    } else {
        0
    };

    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    // Limit buffer transfer size to keep heapless stack footprint safe
    let actual_len = core::cmp::min(buf_len, 256);
    let mut temp_buf = heapless::Vec::<u8, 256>::new();
    unsafe {
        temp_buf.set_len(actual_len);
    }

    // Safely load buffer from user virtual space
    safe_copy_from_user(l0_pa, buf_ptr, actual_len, &mut temp_buf[..actual_len])?;

    // Call inner channel write
    let written_bytes = table.with_channel(hv, Rights::WRITE.bits(), |chan| {
        chan.write(&temp_buf[..actual_len], &[])
    })??;

    Ok(written_bytes)
}
