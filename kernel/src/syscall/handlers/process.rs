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

    if bytes.len() < 32 {
        crate::log_error!("EXEC", "Invalid OHLINK for {}", program_name);
        return Err(Status::InvalidArgs);
    }

    let magic = &bytes[0..4];
    if magic != b"OHLK" {
        crate::log_error!("EXEC", "Invalid magic for {}", program_name);
        return Err(Status::InvalidArgs);
    }

    let entry = u64::from_le_bytes([
        bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13],
    ]) as usize;
    let segment_count = u16::from_le_bytes([bytes[14], bytes[15]]) as usize;

    crate::log_info!(
        "EXEC",
        "Loading {}: entry={:#x}, segments={}",
        program_name,
        entry,
        segment_count
    );

    let (proc, name_str) = match program_name {
        "init" => (crate::task::process::allocate_process("init")?, "init"),
        "devmgr" => (crate::task::process::allocate_process("devmgr")?, "devmgr"),
        "vfs" => (crate::task::process::allocate_process("vfs")?, "vfs"),
        _ => return Err(Status::NotFound),
    };
    let pid = proc.id;

    let mut offset = 32;
    let payload_start = 32 + segment_count * 24;

    for i in 0..segment_count {
        if offset + 24 > bytes.len() {
            crate::log_error!("EXEC", "Segment {} descriptor out of bounds", i);
            return Err(Status::InvalidArgs);
        }
        let virt_addr = u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let file_offset = u64::from_le_bytes([
            bytes[offset + 8],
            bytes[offset + 9],
            bytes[offset + 10],
            bytes[offset + 11],
            bytes[offset + 12],
            bytes[offset + 13],
            bytes[offset + 14],
            bytes[offset + 15],
        ]) as usize;
        let size = u32::from_le_bytes([
            bytes[offset + 16],
            bytes[offset + 17],
            bytes[offset + 18],
            bytes[offset + 19],
        ]) as usize;
        let flags_raw = u32::from_le_bytes([
            bytes[offset + 20],
            bytes[offset + 21],
            bytes[offset + 22],
            bytes[offset + 23],
        ]);

        offset += 24;

        let aligned_vaddr = virt_addr & !(4096 - 1);
        let alignment_offset = virt_addr - aligned_vaddr;
        let aligned_size = (size + alignment_offset + 4095) & !(4095);

        let target_va = proc.root_vmar.base + aligned_vaddr;
        let flags = VmarFlags::from_bits(flags_raw);

        let mut vmo = Vmo::create_with_size(aligned_size)?;
        let segment_payload =
            &bytes[payload_start + file_offset..payload_start + file_offset + size];
        vmo.write(alignment_offset, segment_payload)?;
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
