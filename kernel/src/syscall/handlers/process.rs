use crate::mm::vmar::VmarFlags;
use crate::mm::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use crate::task::thread::Thread;
use shared::status::{Result, Status};
use shared::types::HandleValue;

/// Maximum size of a single OHLINK image that SYSCALL_LOAD_BINARY will
/// materialise into a kernel scratch buffer.  CapsuleOS user programs
/// are typically under 32 KiB; we cap at 128 KiB to leave headroom while
/// still fitting comfortably in the .bss of the kernel image.
const LOAD_BINARY_SCRATCH_SIZE: usize = 128 * 1024;
static mut LOAD_BINARY_SCRATCH: [u8; LOAD_BINARY_SCRATCH_SIZE] =
    [0u8; LOAD_BINARY_SCRATCH_SIZE];

pub fn sys_exit(code: i32) -> ! {
    crate::log_info!("SYSCALL", "Process exited with code {}", code);
    loop {}
}

pub fn sys_exec(table: &HandleTable, program_name: &str) -> Result<()> {
    let (name_static, path_str) = match program_name {
        "init" => ("init", "system/bin/init"),
        "devmgr" => ("devmgr", "system/bin/devmgr"),
        "fileagent" => ("fileagent", "system/bin/fileagent"),
        _ => {
            crate::log_error!("EXEC", "Unknown program: {}", program_name);
            return Err(Status::NotFound);
        }
    };

    let bytes = crate::rootfs::get_file(path_str).ok_or_else(|| {
        crate::log_error!("EXEC", "Program {} not found in rootfs path: {}", program_name, path_str);
        Status::NotFound
    })?;

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
        program_name
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

/// SYSCALL_LOAD_BINARY: load an OHLINK-formatted user-space program
/// from a VMO containing the binary bytes. The new process is created,
/// its segments are mapped, and a thread is scheduled to run it at
/// `entry_point`. Returns the new process id (u64) on success.
///
/// Arguments:
/// - `vmo_handle_raw`: handle to a VMO whose contents are a full OHLINK image
/// - `name_ptr`/`name_len`: optional user-space string for the process name
///
/// This is the foundation for the user-space `loader` service: clients
/// (e.g. `init`) read an OHLINK file from `fileagent` into a VMO via the
/// zero-copy IPC path, then call this syscall to actually create the
/// process. Until the loader service wires up its IPC handler, this
/// syscall is unused but fully functional.
pub fn sys_load_binary(
    table: &HandleTable,
    vmo_handle_raw: u32,
    name_ptr: usize,
    name_len: usize,
) -> Result<u64> {
    use crate::object::handle_table::KernelObject;

    let vmo_hv = HandleValue::new(vmo_handle_raw);

    // Read the full OHLINK image out of the source VMO into a heap-backed
    // VMO. The kernel stack is only 16 KiB (KERNEL_STACK_PAGES=4) so a
    // large stack buffer would overflow and corrupt adjacent trap frames.
    // Instead we allocate a kernel VMO of the same size as the source
    // image, copy bytes into it via Vmo::read, then hand launch_user_program
    // a slice borrowed from the kernel heap mapping.
    let source_size: usize = table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| -> usize {
        vmo.size()
    })?;

    // Bound the kernel scratch buffer by both the source VMO size and the
    // static scratch cap (128 KiB).  If the source is larger than the
    // scratch, we refuse to load it (return InvalidArgs) instead of
    // silently truncating, since that would corrupt the loaded program.
    if source_size > LOAD_BINARY_SCRATCH_SIZE {
        crate::log_error!(
            "LOAD_BINARY",
            "source image {} bytes exceeds scratch cap {} bytes",
            source_size,
            LOAD_BINARY_SCRATCH_SIZE
        );
        return Err(Status::InvalidArgs);
    }

    // Stream the source VMO bytes into a pre-allocated kernel .bss scratch
    // buffer (LOAD_BINARY_SCRATCH, defined at module scope).  Reading in
    // 4 KiB chunks keeps the kernel stack at a small, fixed footprint
    // regardless of how large the user-space binary is.
    let copied_into_scratch: usize = table.with_vmo(vmo_hv, Rights::READ.bits(), |src_vmo| -> usize {
        let mut tmp = [0u8; 4096];
        let mut total = 0usize;
        let scratch_cap = LOAD_BINARY_SCRATCH_SIZE;
        while total < src_vmo.size() && total < scratch_cap {
            let want = core::cmp::min(tmp.len(), core::cmp::min(src_vmo.size(), scratch_cap) - total);
            if src_vmo.read(total, &mut tmp[..want]).unwrap_or(0) == 0 {
                break;
            }
            unsafe {
                core::ptr::copy_nonoverlapping(
                    tmp.as_ptr(),
                    LOAD_BINARY_SCRATCH.as_mut_ptr().add(total),
                    want,
                );
            }
            total += want;
        }
        total
    })?;

    crate::log_info!(
        "LOAD_BINARY",
        "loading program ({} bytes) from vmo_handle={}",
        copied_into_scratch,
        vmo_handle_raw
    );

    let bytes_slice: &[u8] = unsafe { &LOAD_BINARY_SCRATCH[..copied_into_scratch] };
    let name_static: &'static str = "user-prog";
    crate::task::process::Process::launch_user_program(name_static, bytes_slice)?;

    // Recover the pid that was assigned inside launch_user_program so
    // the caller can hold a Process handle if it wants.
    let pid = unsafe {
        crate::task::process::PROCESSES
            .iter()
            .rev()
            .find_map(|slot| slot.as_ref().map(|p| p.id))
            .unwrap_or(0)
    };

    // Hand the caller a Process handle so it can later close, wait, etc.
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let _ = table.add(KernelObject::Process(pid), rights);

    Ok(pid)
}
