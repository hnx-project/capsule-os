use crate::ipc::channel::Channel;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use shared::status::{Result, Status};
use shared::types::HandleValue;

/// Helper function to safely copy data from user virtual space to kernel buffer
pub(crate) fn safe_copy_from_user(l0_pa: usize, src_user_va: usize, len: usize, dest: &mut [u8]) -> Result<()> {
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let mut copied = 0;
    while copied < len {
        let va = src_user_va + copied;
        let pa = crate::arch::translate_user_va(l0_pa, va).ok_or(Status::InvalidArgs)?;
        // `pa_to_kernel_va(pa)` already returns the kernel direct-map VA
        // for the **byte at physical address `pa`** — adding `page_offset`
        // (the user-side intra-page offset) on top would skip past `pa`
        // by that many bytes and read from the wrong page (or from
        // outside the page entirely if the destination is the last
        // page).  The `page_offset` is already accounted for inside
        // `pa` because `translate_user_va` walked the user page table
        // down to the l3 entry that owns the byte at `va`.
        let kernel_va = crate::mm::mmu::pa_to_kernel_va(pa);
        let chunk_len = core::cmp::min(len - copied, 4096 - (va & 0xfff));
        unsafe {
            core::ptr::copy_nonoverlapping(
                kernel_va as *const u8,
                dest.as_mut_ptr().add(copied),
                chunk_len
            );
        }
        copied += chunk_len;
    }
    Ok(())
}

/// Helper function to safely copy data from kernel buffer to user virtual space
pub(crate) fn safe_copy_to_user(l0_pa: usize, src: &[u8], dest_user_va: usize, len: usize) -> Result<()> {
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let mut copied = 0;
    while copied < len {
        let va = dest_user_va + copied;
        let pa = crate::arch::translate_user_va(l0_pa, va).ok_or(Status::InvalidArgs)?;
        // See note in `safe_copy_from_user` — do NOT add the user-side
        // intra-page offset to `pa_to_kernel_va(pa)`.  `pa` already
        // accounts for it, and adding again would land us in the wrong
        // physical page.
        let kernel_va = crate::mm::mmu::pa_to_kernel_va(pa);
        let chunk_len = core::cmp::min(len - copied, 4096 - (va & 0xfff));
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(copied),
                kernel_va as *mut u8,
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

pub fn sys_channel_register(table: &HandleTable, name_ptr: usize, name_len: usize,
                            handle_raw: u32) -> Result<()> {
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

    let mut name_buf = [0u8; 16];
    let actual_len = core::cmp::min(name_len, 16);
    safe_copy_from_user(l0_pa, name_ptr, actual_len, &mut name_buf[..actual_len])?;

    let hv = HandleValue::new(handle_raw);
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let chan_ptr = table.with_channel(hv, rights, |c| c as *mut Channel)?;

    crate::ipc::registry::register_service(&name_buf[..actual_len], chan_ptr)?;
    crate::log_info!("NAMING", "Successfully registered system service: '{}'", core::str::from_utf8(&name_buf[..actual_len]).unwrap_or("unknown"));

    Ok(())
}

pub fn sys_channel_lookup(table: &HandleTable, name_ptr: usize, name_len: usize) -> Result<HandleValue> {
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

    let mut name_buf = [0u8; 16];
    let actual_len = core::cmp::min(name_len, 16);
    safe_copy_from_user(l0_pa, name_ptr, actual_len, &mut name_buf[..actual_len])?;

    // Find the server service registration channel endpoint
    let server_service_chan_ptr = crate::ipc::registry::lookup_service(&name_buf[..actual_len])?;

    // Create a new distinct connection channel pair for communication
    let client_chan = Channel::new()?;
    let server_chan = Channel::new()?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();

    let h_client = table.add(KernelObject::Channel(client_chan), rights)?;
    let h_server = table.add(KernelObject::Channel(server_chan), rights)?;

    let c_client_ptr = table.with_channel(h_client, rights, |c| c as *mut Channel)?;
    let c_server_ptr = table.with_channel(h_server, rights, |c| c as *mut Channel)?;

    table.with_channel(h_client, rights, |c| c.peer = Some(c_server_ptr))?;
    table.with_channel(h_server, rights, |c| c.peer = Some(c_client_ptr))?;

    // Pass h_server directly to the server's registered service endpoint through Handle Passing!
    // This wakes up the server listener and injects the new connection handle to the server's table.
    unsafe {
        (*server_service_chan_ptr).write(&[], &[h_server])?;
    }

    crate::log_info!("NAMING", "Successfully resolved and connected client to service: '{}'", core::str::from_utf8(&name_buf[..actual_len]).unwrap_or("unknown"));

    // Return the client handle endpoint to the caller
    Ok(h_client)
}
