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
    let (name_static, bytes): (&'static str, &'static [u8]) = match program_name {
        "init" => ("init", &include_bytes!("../../../files/init")[..]),
        "devmgr" => ("devmgr", &include_bytes!("../../../files/devmgr")[..]),
        "fileagent" => ("fileagent", &include_bytes!("../../../files/fileagent")[..]),
        _ => {
            crate::log_error!("EXEC", "Unknown program: {}", program_name);
            return Err(Status::NotFound);
        }
    };

    crate::task::process::Process::launch_user_program(name_static, bytes)?;

    // Mark the calling thread (init) as Dead so the scheduler never
    // erets back into it.  Until per-process page tables exist, init
    // and the just-exec'd process would share the same global TTBR0
    // page table; init's linker PC-relative adr/adrp would then point
    // into the new process's segments and trigger spurious faults.
    if let Some(caller) = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() } {
        unsafe { (*caller).state = crate::task::thread::ThreadState::Dead; }
    }

    crate::log_info!(
        "EXEC",
        "{} launched at EL0",
        name_static
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

    if let Some(t) = unsafe { crate::task::scheduler::SCHEDULER.get_thread_ptr(tid) } {
        unsafe { (*t).state = crate::task::thread::ThreadState::Ready; }
        Ok(())
    } else {
        Err(Status::NotFound)
    }
}
