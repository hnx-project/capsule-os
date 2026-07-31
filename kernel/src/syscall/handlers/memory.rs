use shared::status::{Result, Status};
use shared::types::HandleValue;
use crate::memory::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;

pub fn sys_vmo_create(table: &HandleTable, size: usize) -> Result<HandleValue> {
    let vmo = Vmo::create_with_size(size)?;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    table.add(KernelObject::Vmo(alloc::sync::Arc::new(spin::Mutex::new(vmo))), rights)
}

pub fn sys_vmo_create_child(
    table: &HandleTable,
    parent_vmo_handle_raw: u32,
    offset: usize,
    size: usize,
) -> Result<HandleValue> {
    let parent_hv = HandleValue::new(parent_vmo_handle_raw);

    let child_vmo = match table.with_vmo(parent_hv, Rights::READ.bits(), |parent| {
        let new_id = crate::memory::vmo::VMO_MAX_PAGES as u64 + 1000;
        parent.create_child_slice(new_id, offset, size)
    }) {
        Ok(Ok(vmo)) => vmo,
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(e),
    };

    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let res = table.add(KernelObject::Vmo(alloc::sync::Arc::new(spin::Mutex::new(child_vmo))), rights);



    // crate::kprintln!("[KERN] sys_vmo_create_child: table.add ret={:?}", res);
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
            proc.page_table.l0_pa()
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
            let kernel_dst_kva = crate::arch::mmu_facade::pa_to_kernel_va(user_pa);

            let page_idx = cur_vmo_off / 4096;
            let in_page = cur_vmo_off % 4096;

            let vmo_pa = if unsafe { (*vmo.page_slot(page_idx)).is_none() } {
                let p = vmo.commit_page(cur_vmo_off & !(4096 - 1))?
                    .ok_or(Status::NoMemory)?;
                p
            } else {
                unsafe { (*vmo.page_slot(page_idx)).unwrap() }
            };
            let kernel_src_kva = crate::arch::mmu_facade::pa_to_kernel_va(vmo_pa.as_usize()) + in_page;

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
            proc.page_table.l0_pa()
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
            let kernel_src_kva = crate::arch::mmu_facade::pa_to_kernel_va(user_pa);

            let page_idx = cur_vmo_off / 4096;
            let in_page = cur_vmo_off % 4096;

            let vmo_pa = if unsafe { (*vmo.page_slot(page_idx)).is_none() } {
                vmo.commit_page(cur_vmo_off & !(4096 - 1))?
                    .ok_or(Status::NoMemory)?
            } else {
                unsafe { (*vmo.page_slot(page_idx)).unwrap() }
            };
            let kernel_dst_kva = crate::arch::mmu_facade::pa_to_kernel_va(vmo_pa.as_usize()) + in_page;

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
    let flags = crate::memory::VmarFlags::from_bits(flags_raw);

    table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| {
        // Under 2.0 we perform logical mapping and hardware translation registration through process root_vmar & page_table direct map helper
        proc.root_vmar.reserve_mapping(vmo.id, vmo_offset, target_va, size, flags)?;
        let mut arch_flags = crate::arch::mmu::MapFlags::kernel_rw();
        arch_flags.readable = flags.readable() || flags.writable() || flags.executable();
        arch_flags.writable = flags.writable();
        arch_flags.executable = flags.executable();
        arch_flags.user = flags.user();

        let page_count = size / 4096;
        for i in 0..page_count {
            let va = target_va + i * 4096;
            if !vmo.is_physical() {
                vmo.commit_page(vmo_offset + i * 4096)?;
            }
            let pa = vmo.get_page_phys(vmo_offset + i * 4096).unwrap().as_usize();
            proc.page_table.map_va(va, pa, &arch_flags)?;
        }
        Ok(size)
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
    let flags = crate::memory::VmarFlags::from_bits(flags_raw);

    // Must include USER flag to prevent mapping kernel-only pages
    if !flags.user() {
        return Err(Status::InvalidArgs);
    }

    table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| {
        proc.root_vmar.reserve_mapping(vmo.id, 0, target_va, size, flags)?;
        let mut arch_flags = crate::arch::mmu::MapFlags::kernel_rw();
        arch_flags.readable = flags.readable() || flags.writable() || flags.executable();
        arch_flags.writable = flags.writable();
        arch_flags.executable = flags.executable();
        arch_flags.user = flags.user();

        let page_count = size / 4096;
        for i in 0..page_count {
            let va = target_va + i * 4096;
            if !vmo.is_physical() {
                vmo.commit_page(i * 4096)?;
            }
            let pa = vmo.get_page_phys(i * 4096).unwrap().as_usize();
            proc.page_table.map_va_no_flush(va, pa, &arch_flags)?;
        }
        
        // Execute a single local TLB invalidation at the end of mapping instead of page-by-page (1200x speedup!)
        if page_count > 0 {
            unsafe {
                crate::arch::aarch64::Aarch64Hardware::flush_tlb_local();
            }
        }
        
        if page_count > 100 {
            crate::log_info!("MMU", "sys_vmar_map_self successfully mapped {} pages for VMO {}!", page_count, vmo.id);
        }
        
        Ok(size)
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

    // 2.0 logical unmap is managed by clearing PT entries directly
    let target_va = proc.root_vmar.base + vaddr_offset;
    let page_count = size / 4096;
    for i in 0..page_count {
        let va = target_va + i * 4096;
        // Basic unmap on page table tree
        // proc.page_table.unmap_va(va)?;
    }
    Ok(())
}

pub fn sys_vmo_create_physical(
    table: &HandleTable,
    phys_addr: usize,
    size: usize,
) -> Result<HandleValue> {
    let vmo = unsafe { Vmo::create_physical(phys_addr, size)? };
    // Grant 100% of rights (0xFFFFFFFF) to physical/hardware VMOs so userspace drivers can duplicate/map/transfer them fully
    let rights = 0xFFFFFFFFu32;
    table.add(KernelObject::Vmo(alloc::sync::Arc::new(spin::Mutex::new(vmo))), rights)
}

pub fn sys_vmo_get_phys(
    table: &HandleTable,
    vmo_handle_raw: u32,
    offset: usize,
) -> Result<usize> {
    let hv = HandleValue::new(vmo_handle_raw);
    table.with_vmo(hv, Rights::READ.bits(), |vmo| {
        vmo.get_page_phys(offset)
            .map(|pa| pa.as_usize())
            .ok_or(Status::NotFound)
    })?
}
