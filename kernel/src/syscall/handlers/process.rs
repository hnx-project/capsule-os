use crate::mm::vmar::VmarFlags;
use crate::mm::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use crate::task::process::CWD_MAX;
use crate::task::thread::Thread;
use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use shared::types::HandleValue;

/// Maximum size of a single OHLINK image that SYSCALL_LOAD_BINARY will
/// materialise into a kernel scratch buffer.  CapsuleOS user programs
/// are typically under 32 KiB; we cap at 128 KiB to leave headroom while
/// still fitting comfortably in the .bss of the kernel image.
const LOAD_BINARY_SCRATCH_SIZE: usize = 128 * 1024;
static mut LOAD_BINARY_SCRATCH: [u8; LOAD_BINARY_SCRATCH_SIZE] =
    [0u8; LOAD_BINARY_SCRATCH_SIZE];

/// Static name slots for newly-launched processes.  `Process::name` is
/// `&'static str`, so callers (such as `sys_exec`) that receive the
/// process name from EL0 user space copy it into one of these slots and
/// hand the resulting `&'static str` to `launch_user_program`.  8 slots
/// is more than enough for CapsuleOS's current launch sequence (loader,
/// init, devmgr, fileagent, plus a handful of dynamic EL0 programs).
const NAME_SLOT_COUNT: usize = 8;
const NAME_SLOT_CAP: usize = 128;
static mut NAME_SLOTS: [[u8; NAME_SLOT_CAP]; NAME_SLOT_COUNT] =
    [[0u8; NAME_SLOT_CAP]; NAME_SLOT_COUNT];
static mut NAME_SLOT_LEN: [usize; NAME_SLOT_COUNT] = [0usize; NAME_SLOT_COUNT];
static NAME_SLOT_CURSOR: AtomicUsize = AtomicUsize::new(0);

fn intern_name(name: &str) -> Result<&'static str> {
    if name.len() >= NAME_SLOT_CAP {
        return Err(Status::InvalidArgs);
    }
    let idx = NAME_SLOT_CURSOR.fetch_add(1, Ordering::Relaxed) % NAME_SLOT_COUNT;
    unsafe {
        let slot = &mut NAME_SLOTS[idx];
        slot[..name.len()].copy_from_slice(name.as_bytes());
        NAME_SLOT_LEN[idx] = name.len();
        let ptr = slot.as_ptr() as *const u8;
        let len = NAME_SLOT_LEN[idx];
        let bytes = core::slice::from_raw_parts(ptr, len);
        Ok(core::str::from_utf8_unchecked(bytes))
    }
}

pub fn sys_exit(code: i32) -> ! {
    crate::log_info!("SYSCALL", "Process exited with code {}", code);
    loop {}
}

pub fn sys_exec(table: &HandleTable, program_name: &str) -> Result<()> {
    // Resolve the rootfs path. Short names ("init", "devmgr", "fileagent")
    // are mapped to "system/bin/<name>". Anything containing a '/' is
    // treated as an absolute rootfs path (e.g. "system/bin/xxx") and
    // queried verbatim. This lets the loader service dispatch any binary
    // the user names, not just the three hardcoded boot services.
    let mapped: heapless::String<160>;
    let path_str: &str = if program_name.contains('/') {
        program_name
    } else {
        mapped = heapless::String::try_from("system/bin/")
            .ok()
            .and_then(|mut s| {
                s.push_str(program_name).ok()?;
                Some(s)
            })
            .ok_or(Status::InvalidArgs)?;
        mapped.as_str()
    };

    let bytes = crate::rootfs::get_file(path_str).ok_or_else(|| {
        crate::log_error!("EXEC", "Program {} not found in rootfs path: {}", program_name, path_str);
        Status::NotFound
    })?;

    let name_static: &'static str = intern_name(program_name)?;
    crate::task::process::Process::launch_user_program(name_static, bytes)?;

    // Mark the calling thread (init) as Dead so the scheduler never
    // erets back into it.  Until per-process page tables exist, init
    // and the just-exec'd process would share the same global TTBR0
    // page table; init's linker PC-relative adr/adrp would then point
    // into the new process's segments and trigger spurious faults.
    //
    // We then immediately reschedule so the freshly-spawned process
    // runs RIGHT NOW instead of returning through the caller's trap
    // handler (where eret would briefly resume the Dead thread in EL0
    // against a now-stale per-process translation, producing an EC=0x0
    // ELR=0x0 fault).
    if let Some(caller) = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() } {
        unsafe {
            (*caller).state = crate::task::thread::ThreadState::Dead;
            // Park the dead thread at a non-zero halt address inside the
            // user region so a future context-restore (which won't pick
            // Dead threads) still has *some* valid PC / spsr stored —
            // garbage here would later surface as EC=0x0 ELR=0x0 if a
            // tick ever races between sys_exec and the actual switch.
            (*caller).context.elr = 0x1usize as u64; // any non-zero PC
            (*caller).context.spsr = 0x000;           // EL0t, all-masked
        }
    }

    // Trigger the scheduler to immediately swap in the just-launched
    // process.  schedule() never returns when it makes a switch; if it
    // does return it means every other thread is also Dead and the
    // CPU should idle.
    unsafe { crate::task::scheduler::SCHEDULER.schedule(); }

    crate::log_info!(
        "EXEC",
        "{} launched at EL0 (path={})",
        program_name,
        path_str
    );
    Ok(())
}

/// Maximum number of argv entries we will materialise on a new process's
/// user stack.  Most CapsuleOS programs (mkdir, touch, ls, cat, ps, kill,
/// rm, rmdir) take at most two arguments; we round up to 16 for headroom.
const EXECVE_MAX_ARGS: usize = 16;

/// Maximum total bytes occupied by argv string payloads on a new process's
/// user stack.  4 KiB is enough for ~16 short paths plus a comfortable
/// margin for the argv pointer array (16 × 8 = 128 bytes).
const EXECVE_ARG_TOTAL: usize = 4096;

/// SYSCALL_EXECVE: replace the current process image with `path` and pass
/// `argv` to its entry point.
///
/// Arguments:
/// - `path_ptr`/`path_len`: caller-provided NUL-less program path
///   (short names like "ls" are mapped to "system/bin/ls"; absolute
///   paths containing a '/' are queried verbatim against the rootfs).
/// - `argv_ptr`/`argv_count`: array of `argc_count` entries, each a
///   `(u64 ptr, u64 len)` pair, describing the argv strings to copy
///   onto the new process's user stack.  argv[0] is conventionally the
///   program name (or the same as `path`).
///
/// Returns 0 on success or a negative `Status::to_raw()` on error.  On
/// success the caller thread is marked Dead (the new process takes over
/// the slot in the scheduler, see `sys_exec` for the rationale).
pub fn sys_execve(
    table: &HandleTable,
    path_ptr: usize,
    path_len: usize,
    argv_ptr: usize,
    argv_count: usize,
) -> Result<()> {
    if path_ptr == 0 || path_len == 0 {
        return Err(Status::InvalidArgs);
    }
    if argv_count > EXECVE_MAX_ARGS {
        return Err(Status::InvalidArgs);
    }

    let caller_pid = current_process_id()?;
    let caller_proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let caller_l0_pa = caller_proc.l0_user_pa;
    if caller_l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    let mut path_buf = [0u8; 256];
    let copy_path_len = core::cmp::min(path_len, path_buf.len());
    crate::syscall::handlers::ipc::safe_copy_from_user(
        caller_l0_pa,
        path_ptr,
        copy_path_len,
        &mut path_buf[..copy_path_len],
    )?;
    let program_name = core::str::from_utf8(&path_buf[..copy_path_len])
        .map_err(|_| Status::InvalidArgs)?;

    let mut arg_bufs: [[u8; 256]; EXECVE_MAX_ARGS] = [[0u8; 256]; EXECVE_MAX_ARGS];
    let mut arg_lens: [usize; EXECVE_MAX_ARGS] = [0usize; EXECVE_MAX_ARGS];
    let mut total_bytes: usize = 0;

    if argv_count > 0 && argv_ptr == 0 {
        return Err(Status::InvalidArgs);
    }

    for i in 0..argv_count {
        let mut pair = [0u8; 16];
        let pair_off = argv_ptr + i * 16;
        crate::syscall::handlers::ipc::safe_copy_from_user(
            caller_l0_pa,
            pair_off,
            16,
            &mut pair,
        )?;
        let s_ptr = u64::from_le_bytes([
            pair[0], pair[1], pair[2], pair[3],
            pair[4], pair[5], pair[6], pair[7],
        ]) as usize;
        let s_len = u64::from_le_bytes([
            pair[8], pair[9], pair[10], pair[11],
            pair[12], pair[13], pair[14], pair[15],
        ]) as usize;
        if s_len > arg_bufs[i].len() {
            return Err(Status::InvalidArgs);
        }
        if s_len > 0 {
            crate::syscall::handlers::ipc::safe_copy_from_user(
                caller_l0_pa,
                s_ptr,
                s_len,
                &mut arg_bufs[i][..s_len],
            )?;
        }
        arg_lens[i] = s_len;
        total_bytes = total_bytes.saturating_add(s_len);
    }
    if total_bytes + argv_count * 8 > EXECVE_ARG_TOTAL {
        return Err(Status::InvalidArgs);
    }

    let mapped: heapless::String<160>;
    let path_str: &str = if program_name.contains('/') {
        program_name
    } else {
        mapped = heapless::String::try_from("system/bin/")
            .ok()
            .and_then(|mut s| {
                s.push_str(program_name).ok()?;
                Some(s)
            })
            .ok_or(Status::InvalidArgs)?;
        mapped.as_str()
    };

    let bytes = crate::rootfs::get_file(path_str).ok_or_else(|| {
        crate::log_error!("EXEC", "Program {} not found in rootfs path: {}", program_name, path_str);
        Status::NotFound
    })?;

    let name_static: &'static str = intern_name(program_name)?;
    crate::task::process::Process::launch_user_program_with_argv(
        name_static,
        bytes,
        &arg_bufs[..argv_count],
        &arg_lens[..argv_count],
        argv_count,
    )?;

    if let Some(caller) = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() } {
        unsafe {
            (*caller).state = crate::task::thread::ThreadState::Dead;
            (*caller).context.elr = 0x1usize as u64;
            (*caller).context.spsr = 0x000;
        }
    }

    unsafe { crate::task::scheduler::SCHEDULER.schedule(); }

    crate::log_info!(
        "EXEC",
        "{} launched at EL0 with argv[{}] (path={})",
        program_name,
        argv_count,
        path_str
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

/// SYSCALL_GETCWD: copy the calling process's current working directory
/// into the user buffer.  Returns the number of bytes written on success
/// or a negative `Status::to_raw()` on error.
///
/// Arguments:
/// - `buf_ptr`/`buf_len`: caller-provided destination buffer (must be
///   non-null, must be at least `process::CWD_MAX` bytes for the full path)
pub fn sys_getcwd(buf_ptr: usize, buf_len: usize) -> Result<usize> {
    if buf_ptr == 0 {
        return Err(Status::InvalidArgs);
    }
    let caller_pid = current_process_id()?;
    let proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let copy_len = core::cmp::min(proc.cwd_len, buf_len);
    if copy_len == 0 {
        return Ok(0);
    }
    // Translate the user-provided VA through the caller's L0 page table.
    // Writing directly to `buf_ptr` would corrupt the kernel's direct-map
    // alias of that address; we must route the bytes through
    // `safe_copy_to_user` so the destination is the actual user page.
    let l0_pa = proc.l0_user_pa;
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    crate::log_info!(
        "GETCWD",
        "pid={} cwd_len={} buf_len={} copy_len={}",
        caller_pid, proc.cwd_len, buf_len, copy_len
    );
    crate::syscall::handlers::ipc::safe_copy_to_user(
        l0_pa,
        &proc.cwd[..copy_len],
        buf_ptr,
        copy_len,
    )?;
    crate::log_info!(
        "GETCWD",
        "pid={} copied {:?}",
        caller_pid,
        core::str::from_utf8(&proc.cwd[..copy_len]).unwrap_or("?")
    );
    Ok(copy_len)
}

/// SYSCALL_CHDIR: change the calling process's current working directory.
/// The supplied path is treated as a rootfs-relative string (no
/// normalisation against `.` / `..` / symlinks yet).  Returns 0 on success.
pub fn sys_chdir(path_ptr: usize, path_len: usize) -> Result<usize> {
    if path_ptr == 0 || path_len == 0 {
        return Err(Status::InvalidArgs);
    }
    let caller_pid = current_process_id()?;
    let proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let copy_len = core::cmp::min(path_len, proc.cwd.len());
    // Mirror the user-VA read with safe_copy_from_user so the source is
    // the actual user page rather than the kernel's direct-map alias of
    // it (which would silently round-trip bytes through kernel memory
    // and corrupt them when MMU attributes differ).
    let l0_pa = proc.l0_user_pa;
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let mut path_buf = [0u8; CWD_MAX];
    crate::syscall::handlers::ipc::safe_copy_from_user(
        l0_pa,
        path_ptr,
        copy_len,
        &mut path_buf[..copy_len],
    )?;
    proc.cwd[..copy_len].copy_from_slice(&path_buf[..copy_len]);
    proc.cwd_len = copy_len;
    crate::log_info!(
        "CHDIR",
        "process '{}' cwd -> {:?}",
        proc.name,
        core::str::from_utf8(&proc.cwd[..proc.cwd_len]).unwrap_or("?")
    );
    Ok(0)
}

fn current_process_id() -> Result<u64> {
    // `get_current_thread_ptr` is locked by the dispatcher; reading it
    // here from inside SYSCALL_GETCWD / SYSCALL_CHDIR handlers is safe
    // because both run with kernel IRQs still masked.
    unsafe {
        let t = crate::task::scheduler::SCHEDULER
            .get_current_thread_ptr()
            .ok_or(Status::NotFound)?;
        Ok((*t).process_id)
    }
}
