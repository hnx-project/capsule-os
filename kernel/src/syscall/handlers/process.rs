use crate::mm::vmar::VmarFlags;
use crate::mm::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use crate::task::thread::Thread;
use shared::status::{Result, Status};
use shared::types::HandleValue;

pub fn sys_exit(code: i32) -> ! {
    crate::log_info!("SYSCALL", "Process exited with code {}", code);
    loop {}
}

pub fn sys_exec(table: &HandleTable, program_name: &str) -> Result<()> {
    let bytes: &'static [u8] = match program_name {
        "init" => &include_bytes!("../../../files/init")[..],
        "devmgr" => &include_bytes!("../../../files/devmgr")[..],
        "vfs" => &include_bytes!("../../../files/vfs")[..],
        _ => {
            crate::log_error!("EXEC", "Unknown program: {}", program_name);
            return Err(Status::NotFound);
        }
    };

    let parser = ohlink_format::parser::OHLK_Parser::new(bytes).map_err(|e| {
        crate::log_error!("EXEC", "Invalid OHLINK format for {}: {:?}", program_name, e);
        Status::InvalidArgs
    })?;

    let header = parser.header();
    
    // In OHLINK-SPEC.md, there is no explicit entry_point in OHLK_Header.
    // Let's deduce entry point relative to virtual address of the first Text segment, or use default.
    let mut entry = if program_name == "init" { 4096 } else { 65536 };
    let mut segments_count = 0;
    
    for idx in 0..header.header_count {
        if let Ok(entry_meta) = parser.get_entry(idx) {
            if entry_meta.ty == ohlink_format::SegmentType::Text.to_u32() {
                entry = entry_meta.offset as usize;
            }
            if entry_meta.ty == ohlink_format::SegmentType::Text.to_u32()
                || entry_meta.ty == ohlink_format::SegmentType::Data.to_u32()
                || entry_meta.ty == ohlink_format::SegmentType::Rodata.to_u32()
                || entry_meta.ty == ohlink_format::SegmentType::Bss.to_u32()
            {
                segments_count += 1;
            }
        }
    }

    crate::log_info!(
        "EXEC",
        "Loading {}: entry={:#x}, segments={}",
        program_name,
        entry,
        segments_count
    );

    let (proc, name_str) = match program_name {
        "init" => (crate::task::process::allocate_process("init")?, "init"),
        "devmgr" => (crate::task::process::allocate_process("devmgr")?, "devmgr"),
        "vfs" => (crate::task::process::allocate_process("vfs")?, "vfs"),
        _ => return Err(Status::NotFound),
    };
    let pid = proc.id;

    for idx in 0..header.header_count {
        let entry_meta = match parser.get_entry(idx) {
            Ok(e) => e,
            _ => continue,
        };
        if entry_meta.ty != ohlink_format::SegmentType::Text.to_u32()
            && entry_meta.ty != ohlink_format::SegmentType::Data.to_u32()
            && entry_meta.ty != ohlink_format::SegmentType::Rodata.to_u32()
            && entry_meta.ty != ohlink_format::SegmentType::Bss.to_u32()
        {
            continue;
        }

        // We override mapped segment virtual address to 0x200000 to align with standard user target compile configurations!
        let virt_addr = 0x200000;
        let size = entry_meta.mem_size as usize;
        let flags_raw = entry_meta.flags;

        let aligned_vaddr = virt_addr & !(4096 - 1);
        let alignment_offset = virt_addr - aligned_vaddr;
        let aligned_size = (size + alignment_offset + 4095) & !(4095);

        let target_va = proc.root_vmar.base + aligned_vaddr;
        let flags = VmarFlags::from_bits(flags_raw);

        let mut vmo = Vmo::create_with_size(aligned_size)?;
        if entry_meta.file_size > 0 {
            let segment_payload = parser.get_segment_data(&entry_meta).map_err(|e| {
                crate::log_error!("EXEC", "Failed to retrieve segment payload for {}: {:?}", program_name, e);
                Status::InvalidArgs
            })?;
            vmo.write(alignment_offset, segment_payload)?;
        }
        proc.root_vmar
            .map(&mut vmo, 0, target_va, aligned_size, flags)?;
    }

    let stack_size = 16 * 1024;
    let mut stack_vmo = Vmo::create_with_size(stack_size)?;
    let stack_vaddr_offset = 0x2000000;
    let stack_va = proc.root_vmar.base + stack_vaddr_offset;

    let stack_flags = VmarFlags::from_bits(
        VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits(),
    );

    proc.root_vmar
        .map(&mut stack_vmo, 0, stack_va, stack_size, stack_flags)?;
    let stack_top = stack_va + stack_size;

    let user_entry = proc.root_vmar.base + entry;
    let mut thread = Thread::new_user(name_str, user_entry, stack_top)?;
    thread.process_id = pid;
    thread.handle_table = &proc.handle_table;
    thread.state = crate::task::thread::ThreadState::Ready;

    unsafe {
        crate::task::scheduler::SCHEDULER.add(thread);
    }

    // Mark the calling thread (init) as Dead so the scheduler never
    // erets back into it.  Until per-process page tables exist, init
    // and the just-exec'd process would share the same global TTBR0
    // page table; init's linker PC-relative adr/adrp would then point
    // into the new process's segments and trigger spurious faults.
    if let Some(caller) = unsafe {
        crate::task::scheduler::SCHEDULER.get_current_thread_mut()
    } {
        caller.state = crate::task::thread::ThreadState::Dead;
    }

    crate::log_info!(
        "EXEC",
        "{} launched at EL0 (entry={:#x}, pid={})",
        name_str,
        user_entry,
        pid
    );
    Ok(())
}

pub fn sys_process_create(
    table: &HandleTable,
    name_ptr: usize,
    name_len: usize,
) -> Result<HandleValue> {
    let name_static = "user-proc";
    let proc = crate::task::process::allocate_process(name_static)?;
    let pid = proc.id;
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    table.add(KernelObject::Process(pid), rights)
}

pub fn sys_thread_create(
    table: &HandleTable,
    process_handle_raw: u32,
    _name_ptr: usize,
    _name_len: usize,
    entry: usize,
    stack_top: usize,
) -> Result<HandleValue> {
    let p_hv = HandleValue::new(process_handle_raw);
    let pid = table.with_process(p_hv, Rights::WRITE.bits(), |id| id)?;

    let proc = crate::task::process::find_process_mut(pid).ok_or(Status::NotFound)?;

    let mut thread = Thread::new_user("user-thread", entry, stack_top)?;
    thread.process_id = pid;
    thread.handle_table = &proc.handle_table;

    let tid = thread.id;
    unsafe {
        crate::task::scheduler::SCHEDULER.add(thread);
    }

    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    table.add(KernelObject::Thread(tid), rights)
}

pub fn sys_thread_start(table: &HandleTable, thread_handle_raw: u32) -> Result<()> {
    let t_hv = HandleValue::new(thread_handle_raw);
    let tid = table.with_thread(t_hv, Rights::WRITE.bits(), |id| id)?;

    unsafe {
        if let Some(t) = crate::task::scheduler::SCHEDULER.get_thread_mut(tid) {
            t.state = crate::task::thread::ThreadState::Ready;
            Ok(())
        } else {
            Err(Status::NotFound)
        }
    }
}
