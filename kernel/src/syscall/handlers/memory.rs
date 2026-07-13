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
    let parent_hv = HandleValue::new(parent_vmo_handle_raw);
    let child_vmo = table.with_vmo(parent_hv, Rights::READ.bits(), |parent| {
        // We generate a fresh randomized/counter-aligned VMO ID inside
        let new_id = crate::mm::vmo::VMO_MAX_PAGES as u64 + 1000; // Let the atomic counter keep relaxing or provide a valid ID
        parent.create_child_slice(new_id, offset, size)
    })??;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    table.add(KernelObject::Vmo(child_vmo), rights)
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

    table.with_vmo(hv, Rights::READ.bits(), |vmo| {
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
                vmo.commit_page(cur_vmo_off & !(4096 - 1))?
                    .ok_or(Status::NoMemory)?
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
    })?
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
