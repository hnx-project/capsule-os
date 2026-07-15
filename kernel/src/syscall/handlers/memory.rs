use shared::status::{Result, Status};
use shared::types::HandleValue;
use crate::mm::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;

pub fn sys_vmo_create(table: &HandleTable, size: usize) -> Result<HandleValue> {
    let vmo = Vmo::create_with_size(size)?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    table.add(KernelObject::Vmo(vmo), rights)
}

pub fn sys_vmo_create_child(
    table: &HandleTable,
    parent_vmo_handle_raw: u32,
    offset: usize,
    size: usize,
) -> Result<HandleValue> {
    for &b in b"[KERN] sys_vmo_create_child: parent=" {
        crate::arch::console_putchar(b);
    }
    let mut tmp = parent_vmo_handle_raw as usize;
    if tmp == 0 {
        crate::arch::console_putchar(b'0');
    } else {
        let mut buf = [0u8; 16];
        let mut i = 0;
        while tmp > 0 {
            buf[i] = b'0' + (tmp % 10) as u8;
            tmp /= 10;
            i += 1;
        }
        for idx in (0..i).rev() {
            crate::arch::console_putchar(buf[idx]);
        }
    }
    for &b in b" off=" {
        crate::arch::console_putchar(b);
    }
    let mut tmp_off = offset;
    if tmp_off == 0 {
        crate::arch::console_putchar(b'0');
    } else {
        let mut buf = [0u8; 16];
        let mut i = 0;
        while tmp_off > 0 {
            buf[i] = b'0' + (tmp_off % 10) as u8;
            tmp_off /= 10;
            i += 1;
        }
        for idx in (0..i).rev() {
            crate::arch::console_putchar(buf[idx]);
        }
    }
    for &b in b" size=" {
        crate::arch::console_putchar(b);
    }
    let mut tmp_sz = size;
    if tmp_sz == 0 {
        crate::arch::console_putchar(b'0');
    } else {
        let mut buf = [0u8; 16];
        let mut i = 0;
        while tmp_sz > 0 {
            buf[i] = b'0' + (tmp_sz % 10) as u8;
            tmp_sz /= 10;
            i += 1;
        }
        for idx in (0..i).rev() {
            crate::arch::console_putchar(buf[idx]);
        }
    }
    crate::arch::console_putchar(b'\n');

    let parent_hv = HandleValue::new(parent_vmo_handle_raw);
    let child_vmo = table.with_vmo(parent_hv, Rights::READ.bits(), |parent| {
        // We generate a fresh randomized/counter-aligned VMO ID inside
        let new_id = crate::mm::vmo::VMO_MAX_PAGES as u64 + 1000; // Let the atomic counter keep relaxing or provide a valid ID
        parent.create_child_slice(new_id, offset, size)
    })??;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let res = table.add(KernelObject::Vmo(child_vmo), rights);
    
    for &b in b"[KERN] sys_vmo_create_child: table.add ret=" {
        crate::arch::console_putchar(b);
    }
    match &res {
        Ok(hv) => {
            let mut val = hv.get() as usize;
            let mut buf = [0u8; 16];
            let mut i = 0;
            while val > 0 {
                buf[i] = b'0' + (val % 10) as u8;
                val /= 10;
                i += 1;
            }
            for idx in (0..i).rev() {
                crate::arch::console_putchar(buf[idx]);
            }
        }
        Err(e) => {
            for &b in b"Err(" {
                crate::arch::console_putchar(b);
            }
            let mut val = e.to_raw();
            let mut buf = [0u8; 16];
            let mut i = 0;
            while val > 0 {
                buf[i] = b'0' + (val % 10) as u8;
                val /= 10;
                i += 1;
            }
            for idx in (0..i).rev() {
                crate::arch::console_putchar(buf[idx]);
            }
            crate::arch::console_putchar(b')');
        }
    }
    crate::arch::console_putchar(b'\n');
    res
}

pub fn sys_vmo_read(
    table: &HandleTable,
    handle_raw: u32,
    vmo_offset: usize,
    user_dst_va: usize,
    len: usize,
) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let l0_user_pa = if let Some(t) = thread_ptr {
        let proc_id = unsafe { (*t).process_id };
        if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
            proc.l0_user_pa
        } else {
            return Err(Status::InvalidArgs);
        }
    } else {
        return Err(Status::InvalidArgs);
    };

    let res = table.with_vmo(hv, Rights::READ.bits(), |vmo| {
        if vmo_offset >= vmo.size() {
            return Ok(0);
        }
        let real_len = core::cmp::min(len, vmo.size() - vmo_offset);
        let mut copied = 0;

        while copied < real_len {
            let cur_vmo_off = vmo_offset + copied;
            let cur_user_va = user_dst_va + copied;

            let user_pa = crate::arch::translate_user_va(l0_user_pa, cur_user_va)
                .ok_or(Status::InvalidArgs)?;
            let kernel_dst_kva = crate::mm::mmu::pa_to_kernel_va(user_pa);

            let page_idx = cur_vmo_off / 4096;
            let in_page = cur_vmo_off % 4096;
            
            let vmo_pa = if unsafe { (*vmo.page_slot(page_idx)).is_none() } {
                let p = vmo.commit_page(cur_vmo_off & !(4096 - 1))?
                    .ok_or(Status::NoMemory)?;
                p
            } else {
                unsafe { (*vmo.page_slot(page_idx)).unwrap() }
            };
            let kernel_src_kva = crate::mm::mmu::pa_to_kernel_va(vmo_pa.as_usize()) + in_page;

            let page_left_src = 4096 - in_page;
            let page_left_dst = 4096 - (cur_user_va & (4096 - 1));
            let chunk = core::cmp::min(
                core::cmp::min(page_left_src, page_left_dst),
                real_len - copied
            );

            unsafe {
                core::ptr::copy_nonoverlapping(
                    kernel_src_kva as *const u8,
                    kernel_dst_kva as *mut u8,
                    chunk
                );
                
                #[cfg(target_arch = "aarch64")]
                {
                    crate::arch::aarch64::mmu::sync_instruction_cache(kernel_dst_kva, chunk);
                }
            }

            copied += chunk;
        }
        Ok(copied)
    })?;
    res
}

pub fn sys_vmo_write(
    table: &HandleTable,
    handle_raw: u32,
    vmo_offset: usize,
    user_src_va: usize,
    len: usize,
) -> Result<usize> {
    let hv = HandleValue::new(handle_raw);
    
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let l0_user_pa = if let Some(t) = thread_ptr {
        let proc_id = unsafe { (*t).process_id };
        if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
            proc.l0_user_pa
        } else {
            return Err(Status::InvalidArgs);
        }
    } else {
        return Err(Status::InvalidArgs);
    };

    table.with_vmo(hv, Rights::WRITE.bits(), |vmo| {
        if vmo_offset >= vmo.size() {
            return Ok(0);
        }
        let real_len = core::cmp::min(len, vmo.size() - vmo_offset);
        let mut copied = 0;

        while copied < real_len {
            let cur_vmo_off = vmo_offset + copied;
            let cur_user_va = user_src_va + copied;

            let user_pa = crate::arch::translate_user_va(l0_user_pa, cur_user_va)
                .ok_or(Status::InvalidArgs)?;
            let kernel_src_kva = crate::mm::mmu::pa_to_kernel_va(user_pa);

            let page_idx = cur_vmo_off / 4096;
            let in_page = cur_vmo_off % 4096;
            
            let vmo_pa = if unsafe { (*vmo.page_slot(page_idx)).is_none() } {
                vmo.commit_page(cur_vmo_off & !(4096 - 1))?
                    .ok_or(Status::NoMemory)?
            } else {
                unsafe { (*vmo.page_slot(page_idx)).unwrap() }
            };
            let kernel_dst_kva = crate::mm::mmu::pa_to_kernel_va(vmo_pa.as_usize()) + in_page;

            let page_left_src = 4096 - (cur_user_va & (4096 - 1));
            let page_left_dst = 4096 - in_page;
            let chunk = core::cmp::min(
                core::cmp::min(page_left_src, page_left_dst),
                real_len - copied
            );

            unsafe {
                core::ptr::copy_nonoverlapping(
                    kernel_src_kva as *const u8,
                    kernel_dst_kva as *mut u8,
                    chunk
                );
                
                #[cfg(target_arch = "aarch64")]
                {
                    crate::arch::aarch64::mmu::sync_instruction_cache(kernel_dst_kva, chunk);
                }
            }

            copied += chunk;
        }
        Ok(copied)
    })?
}

pub fn sys_vmar_map(
    table: &HandleTable,
    process_handle_raw: u32,
    vmo_handle_raw: u32,
    vmo_offset: usize,
    size: usize,
    vaddr_offset: usize,
    flags_raw: u32,
) -> Result<usize> {
    let p_hv = HandleValue::new(process_handle_raw);
    let pid = table.with_process(p_hv, Rights::WRITE.bits(), |id| id)?;

    let proc = crate::task::process::find_process_mut(pid).ok_or(Status::NotFound)?;
    let vmo_hv = HandleValue::new(vmo_handle_raw);

    let target_va = proc.root_vmar.base + vaddr_offset;
    let flags = crate::mm::vmar::VmarFlags::from_bits(flags_raw);

    table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| {
        proc.root_vmar.map(vmo, vmo_offset, target_va, size, flags)
    })?
}

pub fn sys_vmar_map_self(
    table: &HandleTable,
    vmo_handle_raw: u32,
    vaddr_offset: usize,
    size: usize,
    flags_raw: u32,
) -> Result<usize> {
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
        .ok_or(Status::NotFound)?;
    let proc_id = unsafe { (*thread_ptr).process_id };
    let proc = crate::task::process::find_process_mut(proc_id)
        .ok_or(Status::NotFound)?;

    let vmo_hv = HandleValue::new(vmo_handle_raw);
    let target_va = proc.root_vmar.base + vaddr_offset;
    let flags = crate::mm::vmar::VmarFlags::from_bits(flags_raw);

    // Must include USER flag to prevent mapping kernel-only pages
    if !flags.user() {
        return Err(Status::InvalidArgs);
    }

    table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| {
        proc.root_vmar.map(vmo, 0, target_va, size, flags)
    })?
}

pub fn sys_vmar_unmap(
    vaddr_offset: usize,
    size: usize,
) -> Result<()> {
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
        .ok_or(Status::NotFound)?;
    let proc_id = unsafe { (*thread_ptr).process_id };
    let proc = crate::task::process::find_process_mut(proc_id)
        .ok_or(Status::NotFound)?;

    let target_va = proc.root_vmar.base + vaddr_offset;
    proc.root_vmar.unmap(target_va, size)
}
