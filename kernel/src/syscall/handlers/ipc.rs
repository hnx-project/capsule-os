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
        let kernel_va = crate::arch::mmu_facade::pa_to_kernel_va(pa);
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
        let kernel_va = crate::arch::mmu_facade::pa_to_kernel_va(pa);
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

    let h0 = table.add(KernelObject::Channel(alloc::boxed::Box::new(chan0)), rights)?;
    let h1 = table.add(KernelObject::Channel(alloc::boxed::Box::new(chan1)), rights)?;

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
                        buf_ptr: usize, buf_len: usize,
                        handles_ptr: usize, handles_len: usize) -> Result<usize> {
    let non_blocking = (handle_raw & 0x80000000) != 0;
    let real_handle = handle_raw & 0x7FFFFFFF;
    let hv = HandleValue::new(real_handle);
    
    // Extract current thread's process page table base (L0_PA)
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

    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    // Allocate a temporary kernel-level buffer to receive data from channel safely
    let mut temp_buf = heapless::Vec::<u8, 1024>::new();
    let actual_len = core::cmp::min(buf_len, 1024);
    unsafe {
        temp_buf.set_len(actual_len);
    }

    let actual_h_len = core::cmp::min(handles_len, 2);
    let mut temp_handles = [HandleValue::new(0); 2];

    // Retrieve raw pointer of Channel inside the HandleTable to avoid deadlock on schedule()
    let chan_ptr = table.with_channel(hv, Rights::READ.bits(), |chan| chan as *mut Channel)?;

    if non_blocking {
        let has_sender = unsafe {
            if let Some(peer_ptr) = (*chan_ptr).peer {
                let mut found = false;
                for slot in (*peer_ptr).send_waiters.iter() {
                    if slot.is_some() {
                        found = true;
                        break;
                    }
                }
                found
            } else {
                return Err(Status::PeerClosed);
            }
        };
        if !has_sender {
            return Err(Status::TryAgain);
        }
    }

    // Call inner channel read WITHOUT holding HandleTable lock
    let read_bytes = unsafe {
        (*chan_ptr).read(&mut temp_buf[..actual_len], &mut temp_handles[..actual_h_len])
    }?;

    // Safely write results back to user virtual space
    safe_copy_to_user(l0_pa, &temp_buf[..read_bytes], buf_ptr, read_bytes)?;

    // Safely write returned handles back to user virtual space
    if actual_h_len > 0 && handles_ptr != 0 {
        let mut user_h = [0u32; 2];
        for i in 0..actual_h_len {
            user_h[i] = temp_handles[i].get();
        }
        safe_copy_to_user(
            l0_pa,
            unsafe { core::slice::from_raw_parts(user_h.as_ptr() as *const u8, actual_h_len * 4) },
            handles_ptr,
            actual_h_len * 4,
        )?;
    }

    Ok(read_bytes)
}

pub fn sys_channel_write(table: &HandleTable, handle_raw: u32,
                         buf_ptr: usize, buf_len: usize,
                         handles_ptr: usize, handles_len: usize) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);

    // Extract current thread's process page table base (L0_PA)
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

    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    // Limit buffer transfer size to keep heapless stack footprint safe
    let actual_len = core::cmp::min(buf_len, 1024);
    let mut temp_buf = heapless::Vec::<u8, 1024>::new();
    unsafe {
        temp_buf.set_len(actual_len);
    }

    // Safely load buffer from user virtual space
    safe_copy_from_user(l0_pa, buf_ptr, actual_len, &mut temp_buf[..actual_len])?;

    // Safely copy handles from user space if provided
    let actual_h_len = core::cmp::min(handles_len, 2);
    let mut temp_handles = [HandleValue::new(0); 2];
    if actual_h_len > 0 && handles_ptr != 0 {
        let mut user_h = [0u32; 2];
        safe_copy_from_user(
            l0_pa,
            handles_ptr,
            actual_h_len * 4,
            unsafe { core::slice::from_raw_parts_mut(user_h.as_mut_ptr() as *mut u8, actual_h_len * 4) }
        )?;
        for i in 0..actual_h_len {
            temp_handles[i] = HandleValue::new(user_h[i]);
        }
    }

    // Retrieve raw pointer of Channel inside the HandleTable to avoid deadlock on schedule()
    let chan_ptr = table.with_channel(hv, Rights::WRITE.bits(), |chan| chan as *mut Channel)?;

    // Call inner channel write WITHOUT holding HandleTable lock
    let written_bytes = unsafe {
        (*chan_ptr).write(&temp_buf[..actual_len], &temp_handles[..actual_h_len])
    }?;

    Ok(written_bytes)
}

pub fn sys_channel_register(table: &HandleTable, name_ptr: usize, name_len: usize,
                            handle_raw: u32) -> Result<()> {
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
            proc.page_table.l0_pa()
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

    let h_client = table.add(KernelObject::Channel(alloc::boxed::Box::new(client_chan)), rights)?;
    let h_server = table.add(KernelObject::Channel(alloc::boxed::Box::new(server_chan)), rights)?;

    let c_client_ptr = table.with_channel(h_client, rights, |c| c as *mut Channel)?;
    let c_server_ptr = table.with_channel(h_server, rights, |c| c as *mut Channel)?;

    table.with_channel(h_client, rights, |c| c.peer = Some(c_server_ptr))?;
    table.with_channel(h_server, rights, |c| c.peer = Some(c_client_ptr))?;

    // Write to the PEER of the server channel so the write() rendezvous
    // finds the server's blocked reader on server_service_chan_ptr.recv_waiters.
    unsafe {
        if let Some(peer_ptr) = (*server_service_chan_ptr).peer {
            (*peer_ptr).write(&[], &[h_server])?;
        } else {
            return Err(Status::PeerClosed);
        }
    }

    crate::log_info!("NAMING", "Successfully resolved and connected client to service: '{}'", core::str::from_utf8(&name_buf[..actual_len]).unwrap_or("unknown"));

    // Return the client handle endpoint to the caller
    Ok(h_client)
}
