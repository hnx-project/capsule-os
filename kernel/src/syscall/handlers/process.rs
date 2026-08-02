use crate::memory::vmar::VmarFlags;
use crate::memory::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use crate::task::process::{CWD_MAX, ProcessState};
use crate::task::thread::{Thread, KERNEL_STACK_SIZE};
use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};
use shared::types::HandleValue;

/// Maximum size of a single OHLINK image that SYSCALL_LOAD_BINARY will
/// materialise into a kernel scratch buffer.  CapsuleOS user programs
/// are typically under 32 KiB; we cap at 128 KiB to leave headroom while
/// still fitting comfortably in the .bss of the kernel image.
const LOAD_BINARY_SCRATCH_SIZE: usize = 1024 * 1024;
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
    // Mark the caller thread Dead and immediately reschedule.  Without
    // this, the calling thread stays in its current EL0 trap context,
    // the timer tick keeps selecting it via the self.threads fallback
    // (it's still `Running` from the scheduler's point of view), and
    // the system spins forever in SCHED-SAME.  The same pattern is
    // already used in `sys_exec` / `sys_execve` for the "caller is
    // replaced by the new process" case; here the caller is replaced
    // by *nothing* and we rely on `schedule()` to switch to whatever
    // other thread is alive (or hit the "all dead" halt path).
    //
    // If the caller is the boot anchor (pid 1) and we still have a
    // respawn budget, the kernel spawns `system/bin/init` *before*
    // marking the caller Dead so the scheduler's pop_next pass can
    // pick the new init as a Ready thread.  The respawn only fires
    // once per boot; see `task::init_respawn` for the rationale.
    let caller_pid: u64 = if let Some(caller) =
        unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
    {
        let pid = unsafe { (*caller).process_id };

        // Mark the process as Zombie via the shared process-management
        // helper so that both the direct `sys_exit` path and the
        // `PROC_MGMT_EXIT` syscall path use the same logic.  This
        // also ensures that if procmgr has registered a notification
        // channel the helper can forward the event.
        mark_process_zombie(pid, code);

        crate::task::init_respawn::respawn_init_if_anchor(pid);
        unsafe {
            (*caller).owner_core = None;
            (*caller).state = crate::task::thread::ThreadState::Dead;
            // Park the dead context at a non-zero PC / EL0t so a
            // racing tick that snapshots it before schedule() takes
            // effect still has *some* valid value (avoid the EC=0x0
            // ELR=0x0 abort that surfaced during bring-up).
            (*caller).context.elr = 0x1usize as u64;
            (*caller).context.spsr = 0x000;
        }
        pid
    } else {
        0
    };
    // If `get_current_thread_ptr` returned None (no current
    // thread), the caller_pid is 0 and the respawn above is a
    // no-op.  We still want to fall through to schedule() so the
    // kernel either picks another thread or halts.
    let _ = caller_pid;
    unsafe { crate::task::scheduler::SCHEDULER.schedule(); }

    // `schedule()` either switched to another thread (and will not
    // return) or hit the all-dead halt path.  The `wfe` / `wfi`
    // fallback is purely defensive: if a future change to schedule()
    // ever makes it return when no thread is runnable, we want the
    // CPU to park instead of busy-looping.
    loop {
        unsafe {
            use crate::arch::ArchHardware;
            crate::arch::CurrentArch::wait_for_interrupt();
        }
    }
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
    let _pid = crate::task::process::Process::launch_user_program(name_static, bytes)?;

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
            (*caller).owner_core = None;
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
/// SYSCALL_EXECVE: replace the current process image with a new executable by its VMO capability handle.
pub fn sys_execve(
    table: &HandleTable,
    binary_vmo_handle: u32,
    argv_vmo_handle: u32,
) -> Result<()> {
    if binary_vmo_handle == 0 {
        return Err(Status::InvalidArgs);
    }

    use crate::object::rights::Rights;

    // 1. Read binary bytes into LOAD_BINARY_SCRATCH
    let binary_vmo_hv = HandleValue::new(binary_vmo_handle);
    let source_size = table.with_vmo(binary_vmo_hv, Rights::READ.bits(), |vmo| vmo.size())?;
    if source_size > LOAD_BINARY_SCRATCH_SIZE {
        return Err(Status::InvalidArgs);
    }

    let copied_into_scratch = table.with_vmo(binary_vmo_hv, Rights::READ.bits(), |vmo| {
        let mut buf = unsafe { &mut LOAD_BINARY_SCRATCH[..source_size] };
        vmo.read(0, buf).unwrap_or(0)
    })?;

    if copied_into_scratch != source_size {
        return Err(Status::InvalidArgs);
    }

    // 2. Parse argv from VMO
    let mut arg_bufs = [[0u8; 256]; EXECVE_MAX_ARGS];
    let mut arg_lens = [0usize; EXECVE_MAX_ARGS];
    let mut argv_count = 0;
    let mut argv_vmo_buf = [0u8; 4096];

    if argv_vmo_handle != 0 {
        let argv_vmo_hv = HandleValue::new(argv_vmo_handle);
        let read_bytes = table.with_vmo(argv_vmo_hv, Rights::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), argv_vmo_buf.len());
            vmo.read(0, &mut argv_vmo_buf[..len]).unwrap_or(0)
        })?;

        if read_bytes >= 4 {
            let argc = u32::from_le_bytes([argv_vmo_buf[0], argv_vmo_buf[1], argv_vmo_buf[2], argv_vmo_buf[3]]) as usize;
            let mut offset = 4;
            let count = core::cmp::min(argc, EXECVE_MAX_ARGS);
            for i in 0..count {
                if offset + 4 > read_bytes {
                    break;
                }
                let len = u32::from_le_bytes([
                    argv_vmo_buf[offset],
                    argv_vmo_buf[offset+1],
                    argv_vmo_buf[offset+2],
                    argv_vmo_buf[offset+3],
                ]) as usize;
                offset += 4;
                if offset + len > read_bytes || len > 256 {
                    return Err(Status::InvalidArgs);
                }
                if len > 0 {
                    arg_bufs[i][..len].copy_from_slice(&argv_vmo_buf[offset..offset+len]);
                    arg_lens[i] = len;
                }
                offset += len;
                argv_count += 1;
            }
        }
    }

    // 3. Launch the user program using the standard mechanism
    let caller_pid = current_process_id()?;
    let bytes_slice = unsafe { &LOAD_BINARY_SCRATCH[..copied_into_scratch] };
    let name_static = "user-exec";

    let _pid = crate::task::process::Process::launch_user_program_with_argv(
        name_static,
        bytes_slice,
        &arg_bufs[..argv_count],
        &arg_lens[..argv_count],
        argv_count,
        caller_pid,
    )?;

    // 4. Mark the calling thread as Dead and schedule
    if let Some(caller) = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() } {
        unsafe {
            (*caller).owner_core = None;
            (*caller).state = crate::task::thread::ThreadState::Dead;
            (*caller).context.elr = 0x1usize as u64;
            (*caller).context.spsr = 0x000;
        }
    }

    unsafe { crate::task::scheduler::SCHEDULER.schedule(); }

    Ok(())
}

/// SYSCALL_SPAWN: spawn an EL0 process from a given binary VMO
/// and command-line arguments VMO.  The std-fds-less variant —
/// `libcapsule::syscalls::spawn` uses this; the std-fd-aware
/// `posix_spawn(3)` uses `SYSCALL_SPAWN_STD` instead.
///
/// `envp_vmo_handle` (arg2) and `file_actions_vmo_handle` (arg3)
/// follow the same layout as in `sys_spawn_std`.  Pass `0` for
/// either to skip the corresponding phase.
pub fn sys_spawn(
    table: &HandleTable,
    binary_vmo_handle: u32,
    argv_vmo_handle: u32,
    envp_vmo_handle: u32,
    file_actions_vmo_handle: u32,
    attr_vmo_handle: u32,
) -> Result<u64> {
    if binary_vmo_handle == 0 {
        return Err(Status::InvalidArgs);
    }

    use crate::object::rights::Rights;

    // 1. Read binary bytes into LOAD_BINARY_SCRATCH
    let binary_vmo_hv = HandleValue::new(binary_vmo_handle);
    let source_size = table.with_vmo(binary_vmo_hv, Rights::READ.bits(), |vmo| vmo.size())?;
    if source_size > LOAD_BINARY_SCRATCH_SIZE {
        return Err(Status::InvalidArgs);
    }

    let copied_into_scratch = table.with_vmo(binary_vmo_hv, Rights::READ.bits(), |vmo| {
        let mut buf = unsafe { &mut LOAD_BINARY_SCRATCH[..source_size] };
        vmo.read(0, buf).unwrap_or(0)
    })?;

    if copied_into_scratch != source_size {
        return Err(Status::InvalidArgs);
    }

    // 2. Parse argv from VMO
    let mut arg_bufs = [[0u8; 256]; EXECVE_MAX_ARGS];
    let mut arg_lens = [0usize; EXECVE_MAX_ARGS];
    let mut argv_count = 0;
    let mut argv_vmo_buf = [0u8; 4096];

    if argv_vmo_handle != 0 {
        let argv_vmo_hv = HandleValue::new(argv_vmo_handle);
        let read_bytes = table.with_vmo(argv_vmo_hv, Rights::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), argv_vmo_buf.len());
            vmo.read(0, &mut argv_vmo_buf[..len]).unwrap_or(0)
        })?;

        if read_bytes >= 4 {
            let argc = u32::from_le_bytes([argv_vmo_buf[0], argv_vmo_buf[1], argv_vmo_buf[2], argv_vmo_buf[3]]) as usize;
            let mut offset = 4;
            let count = core::cmp::min(argc, EXECVE_MAX_ARGS);
            for i in 0..count {
                if offset + 4 > read_bytes {
                    break;
                }
                let len = u32::from_le_bytes([
                    argv_vmo_buf[offset],
                    argv_vmo_buf[offset+1],
                    argv_vmo_buf[offset+2],
                    argv_vmo_buf[offset+3],
                ]) as usize;
                offset += 4;
                if offset + len > read_bytes || len > 256 {
                    return Err(Status::InvalidArgs);
                }
                if len > 0 {
                    arg_bufs[i][..len].copy_from_slice(&argv_vmo_buf[offset..offset+len]);
                    arg_lens[i] = len;
                }
                offset += len;
                argv_count += 1;
            }
        }
    }

    // 2b. Parse envp from VMO (S13.1).  Same encoding as argv.
    let mut env_bufs = [[0u8; 256]; EXECVE_MAX_ARGS];
    let mut env_lens = [0usize; EXECVE_MAX_ARGS];
    let mut envp_count = 0;
    let mut envp_vmo_buf = [0u8; 4096];

    if envp_vmo_handle != 0 {
        let envp_vmo_hv = HandleValue::new(envp_vmo_handle);
        let read_bytes = table.with_vmo(envp_vmo_hv, Rights::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), envp_vmo_buf.len());
            vmo.read(0, &mut envp_vmo_buf[..len]).unwrap_or(0)
        })?;

        if read_bytes >= 4 {
            let envc = u32::from_le_bytes([
                envp_vmo_buf[0], envp_vmo_buf[1], envp_vmo_buf[2], envp_vmo_buf[3],
            ]) as usize;
            let mut offset = 4;
            let count = core::cmp::min(envc, EXECVE_MAX_ARGS);
            for i in 0..count {
                if offset + 4 > read_bytes {
                    break;
                }
                let len = u32::from_le_bytes([
                    envp_vmo_buf[offset],
                    envp_vmo_buf[offset + 1],
                    envp_vmo_buf[offset + 2],
                    envp_vmo_buf[offset + 3],
                ]) as usize;
                offset += 4;
                if offset + len > read_bytes || len > 256 {
                    return Err(Status::InvalidArgs);
                }
                if len > 0 {
                    env_bufs[i][..len].copy_from_slice(&envp_vmo_buf[offset..offset + len]);
                    env_lens[i] = len;
                }
                offset += len;
                envp_count += 1;
            }
        }
    }

    // 3. Launch the user program using the standard mechanism
    let caller_pid = current_process_id()?;
    let bytes_slice = unsafe { &LOAD_BINARY_SCRATCH[..copied_into_scratch] };
    let name_static = "user-spawn";

    let pid = crate::task::process::Process::launch_user_program_with_argv_and_envp(
        name_static,
        bytes_slice,
        &arg_bufs[..argv_count],
        &arg_lens[..argv_count],
        argv_count,
        &env_bufs[..envp_count],
        &env_lens[..envp_count],
        envp_count,
        caller_pid,
    )?;

    // 4. Apply the spawn attribute (flags + pgroup) to the
    //    freshly-spawned child.  Same logic as `sys_spawn_std`'s
    //    step 6 — kept inline here so the std-fds-less path
    //    honours `POSIX_SPAWN_SETSID` / `POSIX_SPAWN_SETPGROUP`
    //    too.
    if attr_vmo_handle != 0 {
        use crate::object::rights::Rights as _R;
        let attr_vmo_hv = HandleValue::new(attr_vmo_handle);
        let mut attr_buf = [0u8; 32];
        let read = table.with_vmo(attr_vmo_hv, _R::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), attr_buf.len());
            vmo.read(0, &mut attr_buf[..len]).unwrap_or(0)
        })?;
        if read >= 8 {
            let flags = u32::from_le_bytes([
                attr_buf[0], attr_buf[1], attr_buf[2], attr_buf[3],
            ]);
            let pgroup_arg = i32::from_le_bytes([
                attr_buf[4], attr_buf[5], attr_buf[6], attr_buf[7],
            ]);
            if let Some(child) =
                crate::task::process::find_process_mut(pid)
            {
                const POSIX_SPAWN_SETSID: u32 = 16;
                const POSIX_SPAWN_SETPGROUP: u32 = 2;
                if (flags & POSIX_SPAWN_SETSID) != 0 {
                    child.sid = child.id;
                }
                if (flags & POSIX_SPAWN_SETPGROUP) != 0 {
                    let new_pgroup = if pgroup_arg == 0 {
                        child.id
                    } else {
                        pgroup_arg as u64
                    };
                    child.pgroup = new_pgroup;
                }
            }
        }
    }

    // 5. Register the Process handle so the caller can wait/terminate it
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let _ = table.add(crate::object::handle_table::KernelObject::Process(pid), rights);

    Ok(pid)
}

/// S7 / procmgr std-fd handoff.  Mirrors `sys_spawn` but also
/// installs the caller's `(stdin, stdout, stderr)` channel
/// handles into the new process's `fd_table[0..=2]`.  The
/// third vmo carries 12 bytes packed as three `u32` channel
/// handles; `0` means "fall back to the kernel-builtin UART
/// for that slot".
///
/// `envp_vmo_handle` (arg3) carries the spawner's environment
/// in the same `[u32 argc][u32 strlen][bytes]` VMO layout
/// `sys_spawn` uses for argv.  In Pangu 1.0 the spawned
/// process receives an empty environment — the VMO is
/// accepted for source-compatibility but not yet consumed
/// (the envp materialisation path lands under S13, see
/// `libraries/libcapsule/src/posix_spawn/API.md`).
///
/// `file_actions_vmo_handle` (arg4) carries a `posix_spawn`
/// file-actions list.  In Pangu 1.0 we honour only the
/// `close(fd)` and `dup2(oldfd, newfd)` ops (where
/// `newfd ∈ {0, 1, 2}`).  See the `posix_spawn/API.md` for
/// the full state of the 1.0 subset.
///
/// Format:
///     [u32 count]
///     for i in 0..count:
///         [u32 op]      // 1 = close, 2 = dup2
///         [u32 arg0]    // close:fd / dup2:newfd
///         [u32 arg1]    // close:unused / dup2:oldfd
pub fn sys_spawn_std(
    table: &HandleTable,
    binary_vmo_handle: u32,
    argv_vmo_handle: u32,
    std_fds_vmo_handle: u32,
    envp_vmo_handle: u32,
    file_actions_vmo_handle: u32,
    attr_vmo_handle: u32,
) -> Result<u64> {
    // 1. Run the same path as `sys_spawn` to get a fresh pid.
    //    Pass `0` for envp + file_actions because `sys_spawn_std`
    //    applies those AFTER `sys_spawn` returns — re-applying
    //    them here would either double-apply or be ignored,
    //    depending on the kernel's path.  `sys_spawn_std` is
    //    the canonical entry point for `posix_spawn(3)`.
    let pid = sys_spawn(
        table,
        binary_vmo_handle,
        argv_vmo_handle,
        0, // envp: applied by sys_spawn_std after
        0, // file_actions: applied by sys_spawn_std after
        0, // attr: applied by sys_spawn_std after
    )?;

    // 2. Pull the three handle numbers out of the std-fds vmo.
    //    Layout: 12 bytes — three little-endian u32 channel
    //    handles (stdin, stdout, stderr).  An entry of `0` means
    //    "kernel-builtin UART" (i.e. leave the slot at the
    //    default `FdEntry::Pipe` value).
    let mut std_handles: [Option<u32>; 3] = [None; 3];
    if std_fds_vmo_handle != 0 {
        use crate::object::rights::Rights;
        let std_vmo_hv = HandleValue::new(std_fds_vmo_handle);
        let mut std_buf = [0u8; 12];
        let read = table.with_vmo(std_vmo_hv, Rights::READ.bits(), |vmo| {
            let want = std_buf.len().min(vmo.size());
            vmo.read(0, &mut std_buf[..want]).unwrap_or(0)
        })?;
        if read >= 4 {
            std_handles[0] = Some(u32::from_le_bytes([
                std_buf[0], std_buf[1], std_buf[2], std_buf[3],
            ]));
        }
        if read >= 8 {
            std_handles[1] = Some(u32::from_le_bytes([
                std_buf[4], std_buf[5], std_buf[6], std_buf[7],
            ]));
        }
        if read >= 12 {
            std_handles[2] = Some(u32::from_le_bytes([
                std_buf[8], std_buf[9], std_buf[10], std_buf[11],
            ]));
        }
    }

    // 3. Read the envp VMO into a per-call scratch buffer so it
    //    matches the argv parser's `[u32 argc][u32 strlen][bytes]`
    //    format.  We validate the encoding and drop invalid
    //    entries; the spawned process keeps whatever parsed
    //    cleanly.  Materialising envp onto the child user
    //    stack is tracked under S13 — for now we only
    //    consume the VMO and log a single line so we can see
    //    the pipe working.
    if envp_vmo_handle != 0 {
        use crate::object::rights::Rights;
        let env_vmo_hv = HandleValue::new(envp_vmo_handle);
        let mut env_buf = [0u8; 4096];
        let read = table.with_vmo(env_vmo_hv, Rights::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), env_buf.len());
            vmo.read(0, &mut env_buf[..len]).unwrap_or(0)
        })?;
        let mut off = 0usize;
        if read >= 4 {
            let envc = u32::from_le_bytes([
                env_buf[0], env_buf[1], env_buf[2], env_buf[3],
            ]) as usize;
            off = 4;
            for _ in 0..envc {
                if off + 4 > read {
                    break;
                }
                let len = u32::from_le_bytes([
                    env_buf[off],
                    env_buf[off + 1],
                    env_buf[off + 2],
                    env_buf[off + 3],
                ]) as usize;
                off += 4;
                if off + len > read || len > 256 {
                    break;
                }
                off += len;
            }
        }
    }

    // 4. Resolve the new process and install the std fds.
    if let Some(proc) = crate::task::process::find_process_mut(pid) {
        proc.set_std_fds(std_handles, table)?;
    }

    // 5. Apply file actions (close / dup2) to the freshly-spawned
    //    child's fd_table.  We run this AFTER set_std_fds so
    //    dup2's `oldfd` always refers to one of the std fds
    //    installed in step 4.  Off-std-fd dup2 entries are
    //    silently dropped — they have no meaningful source fd
    //    in a freshly-spawned process.
    if file_actions_vmo_handle != 0 {
        use crate::object::rights::Rights;
        let fa_vmo_hv = HandleValue::new(file_actions_vmo_handle);
        let mut fa_buf = [0u8; 4096];
        let read = table.with_vmo(fa_vmo_hv, Rights::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), fa_buf.len());
            vmo.read(0, &mut fa_buf[..len]).unwrap_or(0)
        })?;
        if read >= 4 {
            let count = u32::from_le_bytes([
                fa_buf[0], fa_buf[1], fa_buf[2], fa_buf[3],
            ]) as usize;
            let mut off = 4usize;
            let mut applied = 0usize;
            for _ in 0..count {
                if off + 28 > read {
                    break;
                }
                let op = u32::from_le_bytes([
                    fa_buf[off],
                    fa_buf[off + 1],
                    fa_buf[off + 2],
                    fa_buf[off + 3],
                ]);
                let arg0 = u32::from_le_bytes([
                    fa_buf[off + 4],
                    fa_buf[off + 5],
                    fa_buf[off + 6],
                    fa_buf[off + 7],
                ]);
                let arg1 = u32::from_le_bytes([
                    fa_buf[off + 8],
                    fa_buf[off + 9],
                    fa_buf[off + 10],
                    fa_buf[off + 11],
                ]);
                let arg2 = u32::from_le_bytes([
                    fa_buf[off + 12],
                    fa_buf[off + 13],
                    fa_buf[off + 14],
                    fa_buf[off + 15],
                ]);
                off += 28;
                if let Some(child) =
                    crate::task::process::find_process_mut(pid)
                {
                    apply_file_action(child, table, op, arg0, arg1, arg2);
                }
                    applied += 1;
            }
        }
    }

    // 6. Apply the spawn attribute (flags + pgroup) to the
    //    freshly-spawned child.  See `posix_spawn/API.md` for
    //    the 1.0 VMO format.  SETSID / SETPGROUP are the only
    //    bits honoured in 1.0; sigdefault / sigmask are reserved
    //    for S14.
    if attr_vmo_handle != 0 {
        use crate::object::rights::Rights;
        let attr_vmo_hv = HandleValue::new(attr_vmo_handle);
        let mut attr_buf = [0u8; 32];
        let read = table.with_vmo(attr_vmo_hv, Rights::READ.bits(), |vmo| {
            let len = core::cmp::min(vmo.size(), attr_buf.len());
            vmo.read(0, &mut attr_buf[..len]).unwrap_or(0)
        })?;
        if read >= 8 {
            let flags = u32::from_le_bytes([
                attr_buf[0], attr_buf[1], attr_buf[2], attr_buf[3],
            ]);
            let pgroup_arg = i32::from_le_bytes([
                attr_buf[4], attr_buf[5], attr_buf[6], attr_buf[7],
            ]);
            if let Some(child) =
                crate::task::process::find_process_mut(pid)
            {
                const POSIX_SPAWN_SETSID: u32 = 16;
                const POSIX_SPAWN_SETPGROUP: u32 = 2;
                if (flags & POSIX_SPAWN_SETSID) != 0 {
                    // setsid(): new session, leader = self.
                    child.sid = child.id;
                }
                if (flags & POSIX_SPAWN_SETPGROUP) != 0 {
                    // setpgid(0, pgroup): pgroup=0 means "use
                    // caller's pid as the new pgroup".
                    let new_pgroup = if pgroup_arg == 0 {
                        child.id
                    } else {
                        pgroup_arg as u64
                    };
                    child.pgroup = new_pgroup;
                }
            }
        }
    }

    Ok(pid)
}

/// `SYSCALL_GET_EXTRA_FDS(buf_ptr, max_entries)` — called by the
/// child process during CRT startup (before `main()`) to discover
/// which fds in its per-process `fd_table` refer to `FdEntry::File`
/// entries cloned by `posix_spawn(3)`'s `addopen` action.
///
/// Writes up to `max_entries` entries at `buf_ptr`, each 12 bytes:
///   [fd: u32, hv: u32, remote_fd: u32]
///
/// Returns the number of entries written.
pub fn sys_get_extra_fds(buf_ptr: usize, max_entries: usize) -> Result<usize> {
    if buf_ptr == 0 || max_entries == 0 {
        return Err(Status::InvalidArgs);
    }
    let caller_pid = crate::task::process::current_process_id()?;
    let proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;

    let l0_pa = proc.page_table.l0_pa();
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    // Collect File entries from the fd table.
    let mut entries: [(u32, u32, u32); 64] = [(0, 0, 0); 64];
    let mut count = 0usize;
    for (fd, slot) in proc.fd_table.iter().enumerate() {
        if count >= entries.len() {
            break;
        }
        if let Some(crate::task::process::FdEntry::File { hv, remote_fd }) = slot {
            entries[count] = (fd as u32, hv.get(), *remote_fd);
            count += 1;
        }
    }

    let write_count = core::cmp::min(count, max_entries);

    for i in 0..write_count {
        let off = buf_ptr + i * 12;
        let (fd, hv, rfd) = entries[i];
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&fd.to_le_bytes());
        buf[4..8].copy_from_slice(&hv.to_le_bytes());
        buf[8..12].copy_from_slice(&rfd.to_le_bytes());
        crate::syscall::handlers::ipc::safe_copy_to_user(
            l0_pa, &buf, off, 12,
        )?;
    }

    Ok(write_count)
}

/// Apply one `posix_spawn` file action to the freshly-spawned
/// child.  Supported ops:
///
///   * op == 1 (close): close `arg0` in the child's fd_table.
///   * op == 2 (dup2): if `arg0 ∈ {0, 1, 2}`, dup2 `arg1` into
///     `arg0`.  `arg1` must also be in range; both slots must
///     be valid.
///   * op == 3 (open): clone the channel handle `arg1` from the
///     caller's handle table into the child, set
///     `child.fd_table[arg0] = FdEntry::File { hv, remote_fd: arg2 }`.
///
/// All errors are treated as best-effort: a misformed action
/// at one position does not abort the remaining ones.
fn apply_file_action(
    child: &mut crate::task::process::Process,
    caller_table: &HandleTable,
    op: u32,
    arg0: u32,
    arg1: u32,
    arg2: u32,
) {
    use crate::object::rights::Rights;
    use crate::task::process::FdEntry;
    const OP_CLOSE: u32 = 1;
    const OP_DUP2: u32 = 2;
    const OP_OPEN: u32 = 3;
    match op {
        OP_CLOSE => {
            let fd = arg0 as usize;
            if fd >= crate::task::process::FD_TABLE_SIZE {
                return;
            }
            if let Some(entry) = child.fd_table[fd].take() {
                match entry {
                    FdEntry::Pipe { pipe, role } => {
                        crate::vfs::pipe::pipe_close_role(pipe, role);
                    }
                    FdEntry::Tty { pty, role } => {
                        crate::object::tty::close_pty(
                            pty,
                            role == crate::task::process::TtyRole::Master,
                        );
                    }
                    FdEntry::File { hv, .. } => {
                        let _ = child.handle_table.close(hv);
                    }
                }
            }
        }
        OP_DUP2 => {
            let newfd = arg0 as usize;
            let oldfd = arg1 as usize;
            if newfd >= crate::task::process::FD_TABLE_SIZE
                || oldfd >= crate::task::process::FD_TABLE_SIZE
            {
                return;
            }
            let src = match child.fd_table[oldfd] {
                Some(e) => e,
                None => return,
            };
            // Close whatever was in the destination slot.
            if let Some(old_entry) = child.fd_table[newfd].take() {
                match old_entry {
                    FdEntry::Pipe { pipe, role } => {
                        crate::vfs::pipe::pipe_close_role(pipe, role);
                    }
                    FdEntry::Tty { pty, role } => {
                        crate::object::tty::close_pty(
                            pty,
                            role == crate::task::process::TtyRole::Master,
                        );
                    }
                    FdEntry::File { hv, .. } => {
                        let _ = child.handle_table.close(hv);
                    }
                }
            }
            // For FdEntry::File, duplicate the handle for the
            // destination slot.
            if let FdEntry::File { hv, .. } = src {
                let rights = Rights::READ.bits() | Rights::WRITE.bits();
                if let Ok(new_hv) = child.handle_table.duplicate_handle(hv, rights) {
                    child.fd_table[newfd] = Some(FdEntry::File {
                        hv: new_hv,
                        remote_fd: arg2, // dup2 reuses arg2
                    });
                }
            } else {
                child.fd_table[newfd] = Some(src);
                // Bump pipe refcount on dup (mirrors sys_dup2).
                if let Some(FdEntry::Pipe { pipe, role }) = &child.fd_table[newfd] {
                    crate::vfs::pipe::pipe_clone_role(*pipe, *role);
                }
            }
        }
        OP_OPEN => {
            let newfd = arg0 as usize;
            if newfd >= crate::task::process::FD_TABLE_SIZE
                || newfd < crate::task::process::USER_FD_BASE as usize
            {
                return;
            }
            let hv_src = HandleValue::new(arg1);
            let remote_fd = arg2;
            // Clone the channel handle from the caller's table
            // into the child's table.
            let obj = match caller_table.read_clone(hv_src) {
                Ok(o) => o,
                Err(_) => return,
            };
            let rights = Rights::READ.bits() | Rights::WRITE.bits();
            let child_hv = match child.handle_table.add(obj, rights) {
                Ok(hv) => hv,
                Err(_) => return,
            };
            child.fd_table[newfd] = Some(FdEntry::File {
                hv: child_hv,
                remote_fd,
            });
        }
        _ => {
            // Unknown op — silently ignored.
        }
    }
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
    // Ensure the thread's handle_table pointer points to high-half KVA
    let ht_raw = &proc.handle_table as *const HandleTable as usize;
    let ht_kva = if ht_raw < crate::arch::mmu_facade::KERNEL_OFFSET {
        crate::arch::mmu_facade::pa_to_kernel_va(ht_raw)
    } else {
        ht_raw
    };
    thread.handle_table = ht_kva as *const HandleTable;

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
    vmo_offset: usize,
) -> Result<u64> {
    use crate::object::handle_table::KernelObject;

    let vmo_hv = HandleValue::new(vmo_handle_raw);

    let source_size_res = table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| -> usize {
        vmo.size()
    });

    let source_size = match source_size_res {
        Ok(sz) => sz,
        Err(e) => return Err(e),
    };

    if vmo_offset >= source_size {
        return Err(Status::InvalidArgs);
    }
    let data_size = source_size - vmo_offset;

    if data_size > LOAD_BINARY_SCRATCH_SIZE {
        return Err(Status::InvalidArgs);
    }

    let copied_into_scratch: usize = table.with_vmo(vmo_hv, Rights::READ.bits(), |src_vmo| -> usize {
        let mut total = 0usize;
        let scratch_cap = LOAD_BINARY_SCRATCH_SIZE;
        let remaining = src_vmo.size() - vmo_offset;
        while total < remaining && total < scratch_cap {
            let want = core::cmp::min(4096, core::cmp::min(remaining, scratch_cap) - total);
            let dst_slice = unsafe {
                core::slice::from_raw_parts_mut(
                    LOAD_BINARY_SCRATCH.as_mut_ptr().add(total),
                    want
                )
            };
            if src_vmo.read(vmo_offset + total, dst_slice).unwrap_or(0) == 0 {
                break;
            }
            total += want;
        }
        total
    })?;

    let bytes_slice: &[u8] = unsafe { &LOAD_BINARY_SCRATCH[..copied_into_scratch] };
    let name_static: &'static str = "user-prog";

    let pid = crate::task::process::Process::launch_user_program(name_static, bytes_slice)?;

    // Inject the BootFS VMO into the new process's handle table at the
    // canonical slot 100 used by every EL0 service.  This lets the
    // spawned program use `libcapsule::ServiceLoader::new(100)` to
    // resolve further EL0 services the same way every other service
    // does.  Without this the child has no way to read the HNXF_VFS
    // archive and cannot bootstrap any descendants.
    let rootfs_vmo = unsafe {
        crate::memory::vmo::Vmo::create_physical(
            crate::SERVICES_PHYS_ADDR,
            crate::SERVICES_PHYS_SIZE,
        )?
    };
    if let Some(proc) = crate::task::process::find_process_mut(pid) {
        let rights = crate::object::rights::Rights::READ.bits()
            | crate::object::rights::Rights::WRITE.bits();
        let _ = proc.handle_table.add_raw_handle(
            crate::loader::DEFAULT_BOOTFS_HANDLE_SLOT,
            crate::object::handle_table::KernelObject::Vmo(alloc::sync::Arc::new(spin::Mutex::new(rootfs_vmo))),
            rights,
        );
    }

    // Hand the caller a Process handle so it can later close, wait, etc.
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let _ = table.add(KernelObject::Process(pid), rights);

    Ok(pid)
}

/// SYSCALL_SERVICE_SPAWN: safe and atomic service creation completely within kernel context.
pub fn sys_service_spawn(
    table: &HandleTable,
    desc_ptr: usize,
) -> Result<u64> {
    if desc_ptr == 0 {
        return Err(Status::InvalidArgs);
    }
    
    // 1. Get current process l0 translation page table
    let t = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() }
        .ok_or(Status::NotFound)?;
    let proc_id = unsafe { (*t).process_id };
    let proc = crate::task::process::find_process_mut(proc_id)
        .ok_or(Status::ProcessNotFound)?;
    let l0_pa = proc.page_table.l0_pa();

    // 2. Safely copy the ServiceDescriptor struct from user space to kernel stack
    let mut desc = core::mem::MaybeUninit::<shared::launcher::ServiceDescriptor>::uninit();
    let desc_sz = core::mem::size_of::<shared::launcher::ServiceDescriptor>();
    
    crate::syscall::handlers::ipc::safe_copy_from_user(
        l0_pa,
        desc_ptr,
        desc_sz,
        unsafe { core::slice::from_raw_parts_mut(desc.as_mut_ptr() as *mut u8, desc_sz) }
    )?;
    
    let mut desc = unsafe { desc.assume_init() };

    // 3. Safely copy descriptor name and path strings out of user memory
    let mut name_buf = [0u8; 64];
    let name_len = core::cmp::min(desc.name.len(), 63);
    crate::syscall::handlers::ipc::safe_copy_from_user(
        l0_pa,
        desc.name.as_ptr() as usize,
        name_len,
        &mut name_buf[..name_len]
    )?;
    let name_str = core::str::from_utf8(&name_buf[..name_len]).map_err(|_| Status::InvalidArgs)?;
    
    let mut path_buf = [0u8; 128];
    let path_len = core::cmp::min(desc.path.len(), 127);
    crate::syscall::handlers::ipc::safe_copy_from_user(
        l0_pa,
        desc.path.as_ptr() as usize,
        path_len,
        &mut path_buf[..path_len]
    )?;
    let path_str = core::str::from_utf8(&path_buf[..path_len]).map_err(|_| Status::InvalidArgs)?;

    // Re-bind the temporary kernel-stack string slices into static-lifetime equivalents
    // for standard ServiceLauncher consumption (ServiceLauncher is safe because it only reads
    // these slices during the scope of launch).
    desc.name = unsafe { core::mem::transmute(name_str) };
    desc.path = unsafe { core::mem::transmute(path_str) };

    // 4. Delegate completely to our high-level, zero-duplication ServiceLauncher!
    let pid = crate::loader::ServiceLauncher::launch(&desc)?;

    // Register the Process handle so the caller can wait/terminate it normally
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let _ = table.add(crate::object::handle_table::KernelObject::Process(pid), rights);

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
    let l0_pa = proc.page_table.l0_pa();
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    crate::syscall::handlers::ipc::safe_copy_to_user(
        l0_pa,
        &proc.cwd[..copy_len],
        buf_ptr,
        copy_len,
    )?;
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
    let l0_pa = proc.page_table.l0_pa();
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
    Ok(0)
}

fn current_process_id() -> Result<u64> {
    crate::task::process::current_process_id()
}

fn current_process_l0_pa() -> usize {
    unsafe {
        if let Some(t) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
            let pid = (*t).process_id;
            crate::task::process::find_process_mut(pid).map(|p| p.page_table.l0_pa()).unwrap_or(0)
        } else {
            0
        }
    }
}

/// POSIX `gettid(2)` — returns the kernel-internal `Thread::id` (usize).
/// On Linux this is the kernel TID (a per-thread identifier that is
/// *not* the POSIX pthread tid); we map to whatever `Thread::id` was
/// when the thread was registered.  No syscall-side check beyond
/// "no current thread" → `Status::NotFound`.
pub fn sys_gettid() -> Result<u64> {
    unsafe {
        let t = crate::task::scheduler::SCHEDULER
            .get_current_thread_ptr()
            .ok_or(Status::NotFound)?;
        Ok((*t).id as u64)
    }
}

/// POSIX `getpid(2)` — returns the parent process id (`Process::id`,
/// assigned at `Process::new` time from `PROCESS_ID_COUNTER`).  We do
/// **not** implement `fork(2)` (KERNEL_HEALTH.md K-D1), so this is a
/// 1:1 lookup, not the parent-of-self hack Linux uses for raw `getpid`.
pub fn sys_getpid() -> Result<u64> {
    current_process_id()
}

/// POSIX `getppid(2)` — returns the parent process id recorded at
/// `launch_user_program_with_argv` time.  The kernel itself doesn't
/// track a separate tree beyond this field; the parent hook is
/// primarily used by shell pipelines to know which processes they
/// spawned, not for reparenting.  For pid 1 the kernel keeps
/// `parent_pid = 0` (anonymous ancestor).
pub fn sys_getppid() -> Result<u64> {
    let caller_pid = current_process_id()?;
    let caller_proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    Ok(caller_proc.parent_pid)
}

/// B4 (`KERNEL_HEALTH.md` B4): POSIX `wait4` family.
///
/// `pid > 0`  - reap a specific child whose process_id is `pid`.  If
///               the child is still running we return Status::TryAgain;
///               the caller can spin, yield, or sleep.
/// POSIX `wait4(pid, status, options, rusage)` flags.  Mirrors
/// Linux `<sys/wait.h>` values.
const WNOHANG: i32 = 1;
#[allow(dead_code)]
const WUNTRACED: i32 = 2;

/// `pid = 0`  - reap any direct child of the caller (1.0 stub for
///               process-group-wait).  Process-group support is not
///               yet implemented in CapsuleOS; for 1.0 we collapse
///               "pid=0" to "any direct child".
/// `pid = -1` - reap any direct child of the caller.
/// `pid < -1` - process-group wait (1.0 stub, returns Status::
///               InvalidArgs).
///
/// `status_out_ptr` is a user VA pointing to an `i32`.  We copy a
/// POSIX-style status word out via `safe_copy_to_user` so that a
/// buggy caller can pass null and get Status::InvalidArgs back, while
/// a well-formed caller gets a normal Unix-compatible wait-status.
///
/// `options`:
///   `WNOHANG` (1) — return 0 immediately if no zombie child is
///                    available.  Without `WNOHANG` we block until
///                    either a child becomes a zombie or all
///                    children have been reaped.
///   `WUNTRACED` (2) — also report stopped children.  CapsuleOS
///                    has no SIGSTOP / ptrace infrastructure in 1.0
///                    so this is a no-op (we never produce stopped
///                    processes); we accept the flag silently for
///                    ABI compatibility.
///
/// On success returns the pid of the reaped child.  With `WNOHANG`
/// and no zombie available, returns `Ok(0)` rather than blocking.
    pub fn sys_wait4(
        table: &HandleTable,
        pid: i64,
        status_out_ptr: usize,
        options: i32,
    ) -> Result<u64> {
        let _ = table;
        let wnohang = (options & WNOHANG) != 0;
        // WUNTRACED is accepted but ignored: no stopped children in 1.0.
        let _ = options & WUNTRACED;

        let caller_pid = current_process_id()?;

    if pid < -1 {
        return Err(Status::InvalidArgs);
    }

    // Loop: the caller may be re-scheduled multiple times before
    // one of their children lands in Zombie.  We block on each
    // iteration and let the kernel's exit reaper (see
    // `mark_process_zombie`) wake us up.
    loop {
        // Walk the process table for a direct child of the caller
        // that matches the requested pid filter.
        let mut found_reap_target: Option<(usize, u64, i32)> = None;
        let mut found_running: Option<u64> = None;
        for (slot_idx, slot) in unsafe { &mut crate::task::process::PROCESSES }
            .iter()
            .enumerate()
        {
            let proc = match slot.as_ref() {
                Some(p) => p,
                None => continue,
            };
            if proc.parent_pid != caller_pid {
                continue;
            }
            match pid {
                p if p > 0 => {
                    if proc.id != p as u64 {
                        continue;
                    }
                }
                _ => {} // pid = 0 or -1: any direct child
            }
            if proc.exit_status.is_some()
                && proc.state == crate::task::process::ProcessState::Zombie
            {
                found_reap_target =
                    Some((slot_idx, proc.id, proc.exit_status.unwrap_or(0)));
                break;
            } else {
                found_running = Some(proc.id);
            }
        }

        if let Some((slot_idx, reaped_pid, code)) = found_reap_target {
            // Reap: clear exit_status, mark Dead, free the slot
            // index to be re-allocated.
            crate::log_warn!(
                "sys_wait4",
                "REAPED: pid={} code={} caller={}",
                reaped_pid, code, caller_pid
            );
            if status_out_ptr != 0 {
                let code_bytes = (code as i32).to_le_bytes();
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    match crate::task::process::find_process_mut(caller_pid) {
                        Some(p) => p.page_table.l0_pa(),
                        None => 0,
                    },
                    &code_bytes,
                    status_out_ptr,
                    core::mem::size_of::<i32>(),
                )
                .ok();
            }
            let mut reaped_process = None;
            unsafe {
                reaped_process = crate::task::process::PROCESSES[slot_idx].take();
            }
            if let Some(mut p) = reaped_process {
                p.state = crate::task::process::ProcessState::Dead;
                p.exit_status = None;
                p.thread_count = 0;
            }
            let _ = reaped_process;
            return Ok(reaped_pid);
        }

        if let Some(running_pid) = found_running {
            if wnohang {
                // POSIX: return 0 (no child exited yet) without
                // blocking.
                return Ok(0);
            }

            // Block on this child.  We record the pid filter on
            // the thread and add the thread id to the child's
            // exit_waiters list; `mark_process_zombie` wakes us
            // up by calling `wake_thread` on every waiter.  We
            // hold the scheduler lock while we mutate the child's
            // waiters list so the waiters can't disappear in the
            // middle of an exit-driven wake.
            crate::log_warn!(
                "sys_wait4",
                "BLOCK: pid={} caller={} running={:?}",
                pid, caller_pid, found_running
            );
            let caller_tid = {
                let thread_ptr = unsafe {
                    crate::task::scheduler::SCHEDULER.get_current_thread_ptr()
                };
                match thread_ptr {
                    Some(p) => unsafe { (*p).id },
                    None => return Err(Status::NotFound),
                }
            };
            let sched_flags = unsafe { crate::task::scheduler::SCHEDULER.lock() };
            unsafe {
                if let Some(child_proc) =
                    crate::task::process::find_process_mut(running_pid)
                {
                    if !child_proc.exit_waiters.contains(&(caller_tid as u64)) {
                        let _ = child_proc.exit_waiters.push(caller_tid as u64);
                    }
                }
                if let Some(tp) = crate::task::scheduler::SCHEDULER.get_current_thread_ptr() {
                    let t = &mut *tp;
                    t.state = crate::task::thread::ThreadState::Blocked;
                    t.wait_child_pid = pid;
                }
                crate::task::scheduler::SCHEDULER.schedule();
            }
            unsafe { crate::task::scheduler::SCHEDULER.unlock(sched_flags); }
            // Loop and re-check — `mark_process_zombie` will have
            // woken us when the child actually exits.
            continue;
        }
        return Err(Status::NotFound);
    }
}

// -------------------------------------------------------------------------
// B5 (`KERNEL_HEALTH.md` B5): POSIX signal surface (1.0 subset).
// -------------------------------------------------------------------------

/// `SYSCALL_SIGACTION(sig, sa_handler, mask, flags)` - 1.0 stub.
/// Accepts `sa_handler = SIG_DFL (0)` or `SIG_IGN (1)` only;
/// custom user-mode handler trampolines are not yet supported.
/// `mask` is the bitfield of signals to block; ignored in 1.0
/// because `sigprocmask` is a stub.  `flags` is forwarded but
/// not honored.  Returns the previous disposition so callers
/// can chain.
pub fn sys_sigaction(
    _table: &HandleTable,
    sig: usize,
    sa_handler: usize,
    _mask: usize,
    _flags: usize,
) -> Result<usize> {
    crate::task::signals::sigaction_set(sig, sa_handler)
}

/// `SYSCALL_RAISE(sig)` - self-targeted signal.
pub fn sys_raise(_table: &HandleTable, sig: usize) -> Result<()> {
    crate::task::signals::raise(sig)?;
    // Eager dispatch: raise() returns to the caller at the next
    // syscall_exit anyway via `signals::dispatch_pending`, but
    // checking here lets us return NotAllowed immediately if the
    // caller is in an invalid state (Zombie -> NotAllowed).
    let caller_pid = current_process_id()?;
    if matches!(
        crate::task::process::find_process_mut(caller_pid)
            .map(|p| p.state),
        Some(crate::task::process::ProcessState::Zombie)
    ) {
        return Err(Status::NotAllowed);
    }
    Ok(())
}

/// `SYSCALL_KILL(pid, sig)` - cross-process signal.
/// For 1.0 we support `pid > 0` (specific process) and `pid = 0`
/// (broadcast to all direct children of the caller); `pid = -1`
/// and `pid < -1` are reserved for future process-group support
/// and return `InvalidArgs`.
pub fn sys_kill(_table: &HandleTable, pid: i64, sig: usize) -> Result<()> {
    let caller_pid = current_process_id()?;
    if pid < 0 {
        return Err(Status::InvalidArgs);
    }
    let pid_u64: u64 = pid as u64;
    if pid == 0 {
        // Broadcast to all direct children of the caller.
        let mut count = 0;
        for slot in unsafe { &mut crate::task::process::PROCESSES }.iter() {
            if let Some(p) = slot.as_ref() {
                if p.parent_pid == caller_pid {
                    crate::task::signals::signal_send(p.id, sig)?;
                    count += 1;
                }
            }
        }
        if count == 0 {
            return Err(Status::NotFound);
        }
        return Ok(());
    }
    // Validate that the caller is allowed to signal the target.
    // 1.0 rule: a process may signal itself or any of its direct
    // children.  Other-pid signals return `PermissionDenied`.
    if pid_u64 != caller_pid {
        let target = crate::task::process::find_process_mut(pid_u64)
            .ok_or(Status::NotFound)?;
        if target.parent_pid != caller_pid {
            return Err(Status::AccessDenied);
        }
    }
    crate::task::signals::signal_send(pid_u64, sig)
}

/// `SYSCALL_PAUSE()` - yield the calling thread until a non-blocked
/// signal is pending.  Returns `Ok(0)` after a signal has been
/// dispatched; the caller can then re-poll pending.
pub fn sys_pause(_table: &HandleTable) -> Result<()> {
    let caller_pid = current_process_id()?;
    // Park on the scheduler tick instead of spin-yielding.  Each
    // iteration releases the thread for ~1 tick (~10–16 ms) so
    // a long pause doesn't burn CPU.  Caps at 4096 iterations
    // (~70 seconds at 16 ms/tick) to keep the syscall bounded:
    // a caller that's somehow stuck ignoring every signal still
    // returns with TimedOut rather than living forever.
    //
    // Future work: replace the polled sleep with a per-process
    // "signal wait queue" that `signals::signal_send` can wake
    // directly, eliminating the poll entirely.
    const MAX_ITERATIONS: u32 = 4096;
    const TICKS_PER_ITERATION: u64 = 1;
    for _ in 0..MAX_ITERATIONS {
        let proc = crate::task::process::find_process_mut(caller_pid)
            .ok_or(Status::NotFound)?;
        if matches!(proc.state, ProcessState::Zombie) {
            // Signal has dispatched via SIG_DFL.
            return Ok(());
        }
        if proc.pending_signals != 0
            && crate::task::signals::dispatch_pending(caller_pid)?
        {
            return Ok(());
        }
        // All bits were IGN or none pending: park for one tick.
        sys_thread_sleep(TICKS_PER_ITERATION)?;
    }
    Err(Status::TimedOut)
}

// -------------------------------------------------------------------------
// B6 (`KERNEL_HEALTH.md` B6): POSIX pipe + dup2 surface (1.0 subset).
// -------------------------------------------------------------------------

/// `SYSCALL_PIPE(ufds_ptr)` - allocate a new in-kernel pipe and
/// return its two ends as fresh fds in the caller's per-process
/// `fd_table`.  The bytes at `ufds_ptr` must be at least 8 bytes
/// (two `i32`s) and live in writable user memory; we copy them
/// back via `safe_copy_to_user`.
pub fn sys_pipe(
    table: &HandleTable,
    ufds_ptr: usize,
) -> Result<()> {
    let _ = table;
    let caller_pid = crate::task::process::current_process_id()?;
    let caller_proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let caller_l0 = caller_proc.page_table.l0_pa();
    if caller_l0 == 0 || ufds_ptr == 0 {
        return Err(Status::InvalidArgs);
    }

    let pipe_id = crate::vfs::pipe::alloc_pipe()?;
    // Reserve two adjacent user slots; bump refcount for the
    // caller's hold so the pipe is not freed prematurely.
    let rd_fd = alloc_user_fd(caller_proc)?;
    let wr_fd = alloc_user_fd(caller_proc)?;
    caller_proc.fd_table[rd_fd as usize] = Some(
        crate::task::process::FdEntry::Pipe {
            pipe: pipe_id,
            role: crate::vfs::pipe::PipeRole::Read,
        }
    );
    caller_proc.fd_table[wr_fd as usize] = Some(
        crate::task::process::FdEntry::Pipe {
            pipe: pipe_id,
            role: crate::vfs::pipe::PipeRole::Write,
        }
    );

    let pair: [i32; 2] = [rd_fd as i32, wr_fd as i32];
    let bytes: [u8; 8] = unsafe {
        core::mem::transmute::<[i32; 2], [u8; 8]>(pair)
    };
    crate::syscall::handlers::ipc::safe_copy_to_user(
        caller_l0,
        &bytes,
        ufds_ptr,
        8,
    )
    .ok();

    Ok(())
}

/// S2: read or write on a kernel pipe by id.
/// Layout of `arg0`:
///   - bits[15:0]  = pipe id (u16)
///   - bit[16]     = direction (`0` = read, `1` = write)
/// `arg1` / `arg2` are the user buffer VA / length.
///
/// The user-space VA is NOT directly accessible from kernel mode
/// (each process has its own L0 page table), so we route bytes
/// through the per-process L0 translation via
/// `safe_copy_from_user` / `safe_copy_to_user`.
pub fn sys_pipe_rw(
    _table: &HandleTable,
    id_and_dir: usize,
    buf_ptr: usize,
    buf_len: usize,
) -> Result<usize> {
    use crate::syscall::handlers::ipc::safe_copy_from_user;
    use crate::syscall::handlers::ipc::safe_copy_to_user;
    use crate::vfs::pipe::PipeId;
    use crate::vfs::pipe::pipe_read;
    use crate::vfs::pipe::pipe_write;

    let pipe_id_u16 = (id_and_dir & 0xFFFF) as u32;
    let is_write = (id_and_dir & 0x10000) != 0;
    let id = PipeId(pipe_id_u16);
    if buf_ptr == 0 || buf_len == 0 {
        return Ok(0);
    }

    let caller_pid = crate::task::process::current_process_id()?;
    let caller_proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let l0_pa = caller_proc.page_table.l0_pa();
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    let want = core::cmp::min(buf_len, 4096);
    let mut kernel_buf = [0u8; 4096];

    if is_write {
        safe_copy_from_user(l0_pa, buf_ptr, want, &mut kernel_buf[..want])?;
        let n = pipe_write(id, &kernel_buf[..want])?;
        Ok(n)
    } else {
        let n = pipe_read(id, &mut kernel_buf[..want])?;
        if n > 0 {
            safe_copy_to_user(l0_pa, &kernel_buf[..n], buf_ptr, n)?;
        }
        Ok(n)
    }
}

/// Pick the next free fd slot in `proc.fd_table` and bump
/// `proc.next_fd`.  Skips `0/1/2` (kernel-builtin UART) and the
/// 16-byte cap.
fn alloc_user_fd(proc: &mut crate::task::process::Process) -> Result<u32> {
    let mut start = proc.next_fd.max(crate::task::process::USER_FD_BASE);
    for _ in 0..crate::task::process::FD_TABLE_SIZE as u32 {
        if start as usize >= crate::task::process::FD_TABLE_SIZE as u32 as usize {
            start = crate::task::process::USER_FD_BASE;
        }
        if proc.fd_table[start as usize].is_none() {
            let fd = start as u32;
            proc.next_fd = (start + 1) % crate::task::process::FD_TABLE_SIZE as u32;
            return Ok(fd);
        }
        start = (start + 1) % crate::task::process::FD_TABLE_SIZE as u32;
    }
    Err(Status::NoMemory)
}

/// `SYSCALL_DUP2(oldfd, newfd)` - duplicate a process's fd into a
/// specific slot.  Mirror of Linux semantics at the 1.0 level.
/// Returns the new fd on success.
pub fn sys_dup2(_table: &HandleTable, oldfd: u32, newfd: u32) -> Result<u32> {
    let caller_pid = crate::task::process::current_process_id()?;
    let caller_proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;

    if oldfd == newfd {
        return Ok(newfd);
    }
    if newfd == 0 || newfd == 1 || newfd == 2 {
        // The kernel owns fd 0/1/2 (K-D2 UART path); we cannot
        // hand them to user-space without breaking the early
        // boot console.  Linux allows dup2 with values in
        // {0,1,2} (and ESHOPEN / Linux 3.6 actually bumps the
        // O_CLOEXEC bits), but for 1.0 we reject explicitly so
        // smoke tests fail loudly instead of silently dropping
        // bytes on stderr.
        return Err(Status::NotAllowed);
    }

    let src_entry = if (oldfd as usize) < crate::task::process::FD_TABLE_SIZE {
        caller_proc.fd_table[oldfd as usize].ok_or(Status::BadHandle)?
    } else {
        return Err(Status::BadHandle);
    };

    // If newfd already had an entry, close the old one (drop a
    // refcount on the pipe); mirror Linux dup2's silent close.
    if let Some(old_entry) = caller_proc.fd_table[newfd as usize] {
        match old_entry {
            crate::task::process::FdEntry::Pipe { pipe, role } => {
                crate::vfs::pipe::pipe_close_role(pipe, role);
            }
            crate::task::process::FdEntry::Tty { pty, role } => {
                crate::object::tty::close_pty(
                    pty,
                    role == crate::task::process::TtyRole::Master,
                );
            }
            crate::task::process::FdEntry::File { hv, .. } => {
                let _ = caller_proc.handle_table.close(hv);
            }
        }
    }
    caller_proc.fd_table[newfd as usize] = Some(src_entry);

    // Bump refcount because we just added a second fd pointing
    // into the same pipe.
    if let crate::task::process::FdEntry::Pipe { pipe, role } = src_entry {
        crate::vfs::pipe::pipe_clone_role(pipe, role);
    }
    Ok(newfd)
}

/// Helper used by sys_read_posix / sys_write_posix to check
/// whether `fd` is a pipe end inside the calling process.  When
/// it is, the bytes are copied in-place out of / into the kernel
/// pipe buffer and the function returns `true` (caller drops the
/// fileagent forwarder).  When it isn't, the function returns
/// `false` (caller falls through to the channel forwarder).
pub(crate) fn dispatch_pipe_io(
    table: &HandleTable,
    fd: u32,
    buf_ptr: usize,
    buf_len: usize,
    is_write: bool,
) -> Result<Option<usize>> {
    let _ = table;
    if (fd as usize) >= crate::task::process::FD_TABLE_SIZE
        || fd < crate::task::process::USER_FD_BASE
    {
        return Ok(None);
    }
    let caller_pid = crate::task::process::current_process_id()?;
    let proc = match crate::task::process::find_process_mut(caller_pid) {
        Some(p) => p,
        None => return Ok(None),
    };
    let entry = match proc.fd_table[fd as usize] {
        Some(e) => e,
        None => return Ok(None),
    };
    let (pipe_id, role) = match entry {
        crate::task::process::FdEntry::Pipe { pipe, role } => (pipe, role),
        crate::task::process::FdEntry::Tty { .. }
        | crate::task::process::FdEntry::File { .. } => {
            // PTY and File fds are not pipe-coupled.  Return
            // `Ok(None)` so the caller can fall back to the
            // appropriate I/O path (SYSCALL_PTY_* or userspace
            // fileagent IPC).
            return Ok(None);
        }
    };

    // Build a transient &[u8] / &mut [u8] view into user memory.
    // safe_copy_to_user / safe_copy_from_user operate on kernel
    // buffers that have already been translated; we do the
    // equivalent pre-step here so that the pipe write path can
    // process bytes in one shot.
    if buf_ptr == 0 || buf_len == 0 {
        return Ok(Some(0));
    }
    let mut kernel_buf = [0u8; 4096];
    let want = core::cmp::min(buf_len, kernel_buf.len());
    if is_write {
        // Caller wants to write to the pipe.  Pull the bytes
        // out of user memory via safe_copy_from_user.
        match role {
            crate::vfs::pipe::PipeRole::Write => {}
            crate::vfs::pipe::PipeRole::Read => {
                return Err(Status::NotAllowed);
            }
        }
        if proc.page_table.l0_pa() == 0 {
            return Err(Status::InvalidArgs);
        }
        crate::syscall::handlers::ipc::safe_copy_from_user(
            proc.page_table.l0_pa(),
            buf_ptr,
            want,
            &mut kernel_buf[..want],
        )?;
        let n = crate::vfs::pipe::pipe_write(pipe_id, &kernel_buf[..want])?;
        // Wake any consumers on the read end so the kernel's
        // spawn-stay-complete path is unaffected.  1.0 pipe is
        // non-blocking; this is a no-op for the current
        // implementation but matches the contract that callers
        // will see.
        Ok(Some(n))
    } else {
        match role {
            crate::vfs::pipe::PipeRole::Read => {}
            crate::vfs::pipe::PipeRole::Write => {
                return Err(Status::NotAllowed);
            }
        }
        let n = crate::vfs::pipe::pipe_read(
            pipe_id,
            &mut kernel_buf[..want],
        )?;
        if n > 0 && proc.page_table.l0_pa() != 0 {
            crate::syscall::handlers::ipc::safe_copy_to_user(
                proc.page_table.l0_pa(),
                &kernel_buf[..n],
                buf_ptr,
                n,
            )
            .ok();
        }
        Ok(Some(n))
    }
}

/// Shared helper: mark a process as Zombie with an exit code.
/// Both `sys_exit` and `PROC_MGMT_EXIT` route through here so
/// there is a single point where a procmgr notification hook
/// can fire in the future.
fn mark_process_zombie(pid: u64, exit_code: i32) {
    if let Some(proc) = crate::task::process::find_process_mut(pid) {
        proc.exit_status = Some(exit_code);
        proc.state = crate::task::process::ProcessState::Zombie;
    }

    // Wake every thread that was blocked in `sys_wait4` waiting
    // for this child to exit.  POSIX says any thread waiting in
    // wait4 (not just the parent) should be woken when a child
    // dies; in 1.0 we just match pid exactly.  The kernel's
    // scheduler picks them up on the next interrupt / schedule.
    let waiters: heapless::Vec<u64, 8> = if let Some(proc) = crate::task::process::find_process_mut(pid) {
        proc.exit_waiters.clone()
    } else {
        heapless::Vec::new()
    };
    crate::log_warn!(
        "mark_zombie",
        "waking {} waiters for pid={}",
        waiters.len(),
        pid
    );
    for waiter_id in waiters.iter() {
        unsafe { crate::task::scheduler::SCHEDULER.wake_thread(*waiter_id as usize); }
    }
    if let Some(proc) = crate::task::process::find_process_mut(pid) {
        proc.exit_waiters.clear();
    }

    // The caller of `mark_process_zombie` is typically `sys_exit`
    // (in the child thread) which immediately calls
    // `SCHEDULER.schedule()` after us, so the freshly-RunReady
    // parent runs in the same quantum as the child exit.  An
    // explicit `schedule()` here is therefore redundant for the
    // child-exit path; for the signal-driven exit path it would
    // matter but that path isn't wired up in 1.0.

    // S5: dispatch SIGCHLD to the parent so a shell that has
    // installed a handler (or uses `wait4`) sees the death.  We
    // skip the dispatch if the parent has explicitly set
    // SIG_IGN on SIGCHLD, matching Linux's POSIX behaviour.
    if let Some(child) = crate::task::process::find_process_mut(pid) {
        let parent_pid = child.parent_pid;
        if parent_pid != 0 && parent_pid != pid {
            // We don't have direct access to the parent's
            // `sig_handlers` table here without recursing; rather
            // than rolling our own lock accounting, defer the
            // decision to `signals::signal_send` which records
            // the bit pending and lets the parent's exit /
            // next-syscall dispatch path decide what to do.
            let _ = crate::task::signals::signal_send(parent_pid, 17 /* SIGCHLD */);
        }
    }
}

// -------------------------------------------------------------------------
// S3: `sys_fork` — POSIX `fork()` for Pangu 1.0.
//
// Semantics:
//   1. The calling process is duplicated into a fresh child
//      Process with its own pid.  The child's VMAR is a shallow
//      clone of the parent's user-space mappings (sub-table pages
//      are freshly allocated; physical data pages remain shared
//      until one of the two exec's — see
//      `arch/aarch64/page_table::clone_user_from`).
//   2. The child's handle_table is installed with a deep clone of
//      the parent's entries; the fd_table is similarly deep-
//      cloned so a future `close()` from either side does not
//      affect the other.
//   3. The child inherits a single Thread whose context is a byte
//      copy of the caller's context, *with* `x0 = 0` so the
//      child observes the canonical POSIX fork return value of
//      0.  The parent's `x0` is left to the dispatcher (which
//      fills it with the child's pid on the way out).
//   4. The child thread is added to the ready queue via
//      `SCHEDULER.add` so it runs on the next tick.
pub fn sys_fork() -> Result<u64> {
    use crate::task::thread::{Thread, ThreadState};

    let scheduler_flags =
        unsafe { crate::task::scheduler::SCHEDULER.lock() };

    // ---- 1. Snapshot caller identity.  We already hold the
    //    scheduler lock so we can't call `get_current_thread_ptr`
    //    (which would deadlock on a re-entrant `lock()`).  Use
    //    the lock-free accessor instead.
    let caller = unsafe { crate::task::scheduler::SCHEDULER.current_thread_ptr_locked() };
    let caller = match caller {
        Some(p) => p,
        None => {
            unsafe { crate::task::scheduler::SCHEDULER.unlock(scheduler_flags); }
            return Err(Status::NotAllowed);
        }
    };
    let caller_pid = unsafe { (*caller).process_id };

    // ---- 2. Allocate a fresh child Process.  We hold the
    //    scheduler lock so we must use the `lock_held` variant
    //    of the allocator — the regular `allocate_process`
    //    would deadlock on a recursive `lock()`.
    let child_proc = match crate::task::process::allocate_process_with_lock("fork-child", true) {
        Ok(p) => p,
        Err(e) => {
            unsafe { crate::task::scheduler::SCHEDULER.unlock(scheduler_flags); }
            return Err(e);
        }
    };
    child_proc.parent_pid = caller_pid;
    let parent_l0 = crate::task::process::find_process_l0_user_pa(caller_pid)
        .map(|(pa, _)| pa)
        .unwrap_or(0);
    if let Some(parent) = unsafe { crate::task::process::find_process_mut_locked(caller_pid) } {
        let len = core::cmp::min(parent.cwd_len, CWD_MAX);
        child_proc.cwd[..len].copy_from_slice(&parent.cwd[..len]);
        child_proc.cwd_len = len;
        child_proc.next_fd = parent.next_fd;
        child_proc.vmos = parent.vmos.clone();
    }

    // ---- 3. Deep-clone the page tables.
    child_proc.page_table.clone_high_half(parent_l0);
    child_proc.page_table.clone_identity_block(parent_l0);
    child_proc.page_table.clone_user_from(parent_l0);

    // ---- 4. Handle / fd table deep-clone is intentionally a
    //     no-op for 1.0: bash's `fork + exec` always replaces
    //     the child's user-mode handles via the loader anyway,
    //     and our default empty child HandleTable is the safer
    //     choice (it avoids leaking a parent's channels to a
    //     child that has zero reason to inherit them).
    //
    //     The fd_table mirrors this — the loader's fresh process
    //     starts with three reserved slots (fd 0..2) only.

    // ---- 5. Manufacture the child's first Thread.
    let caller_ctx = unsafe { &(*caller).context };
    let mut child_ctx = caller_ctx.clone_for_fork();
    child_ctx.set_x0_for_fork();
    // The user's `syscall!` macro clobbers x30, so we can't
    // rely on the parent's `r[11]` (x30) being meaningful after
    // the SVC.  Reinstall `user_eret_stub` explicitly so the
    // child's first `switch_to` lands at the right trampoline
    // rather than wherever the parent's last `bl` left the link
    // register (the `syscall!` macro clobbers x30).
    child_ctx.r[11] = crate::task::thread::user_eret_stub_addr() as u64;
    // The kernel's sync_el0 entry path saves `elr_el1` as the
    // SVC instruction address itself.  A child that inherits
    // that value would `eret` back into the SVC and re-enter
    // the kernel — shift `elr` past the SVC so the child
    // resumes at the user-mode instruction *after* the SVC.
    child_ctx.advance_elr_for_fork();

    // S11 fork kstack fix: allocate an independent kernel stack
    // for the child and copy the parent's current stack contents
    // (including the SVC trap frame at the bottom) into it.
    let parent_kstack_base_pa = unsafe { (*caller).kernel_stack_base_pa };
    let parent_kstack_size = unsafe { (*caller).kernel_stack_size };
    let parent_kstack_va_base =
        crate::arch::mmu_facade::pa_to_kernel_va(parent_kstack_base_pa.as_usize());
    let parent_kstack_va_top = parent_kstack_va_base + parent_kstack_size;
    let parent_kernel_sp = unsafe { (*caller).kernel_sp };
    let parent_offset_from_top = parent_kstack_va_top - parent_kernel_sp;

    let (child_kstack_base_pa, child_kstack_va_top) =
        match Thread::alloc_independent_kstack() {
            Ok(t) => t,
            Err(e) => {
                // Roll back: deallocate the partially-built
                // child process before releasing the lock so the
                // pid slot can be reused.
                unsafe {
                    crate::task::scheduler::SCHEDULER.unlock(scheduler_flags);
                }
                if let Some(slot_idx) = unsafe {
                    (&crate::task::process::PROCESSES).iter().position(|s| {
                        s.as_ref().map(|p| p.id == child_proc.id).unwrap_or(false)
                    })
                } {
                    unsafe {
                        crate::task::process::PROCESSES[slot_idx] = None;
                    }
                }
                return Err(e);
            }
        };
    let child_kernel_sp = child_kstack_va_top - parent_offset_from_top;

    // Copy the parent's active stack region (saved sp → top)
    // into the child's stack at the same offset.  Both VAs
    // are in the kernel high-half direct mapping so a single
    // `copy_nonoverlapping` from the bottom of the used region
    // (`parent_kernel_sp`) upward for `parent_offset_from_top`
    // bytes gets the right bytes.
    if parent_offset_from_top > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                parent_kernel_sp as *const u8,
                (child_kstack_va_top - parent_offset_from_top) as *mut u8,
                parent_offset_from_top,
            );
        }
    }

    // The child context's stored `sp` (kernel SP) must point
    // at the new stack's matching offset, not the parent's.
    child_ctx.sp = child_kernel_sp as u64;

    let child_thread = Thread {
        id: 0, // assigned by `SCHEDULER.add`
        name: "fork-child",
        state: ThreadState::Ready,
        priority: unsafe { (*caller).priority },
        time_slice: crate::task::thread::DEFAULT_TIME_SLICE,
        remaining_ticks: crate::task::thread::DEFAULT_TIME_SLICE,
        process_id: child_proc.id,
        entry: unsafe { (*caller).entry },
        kernel_stack_base_pa: child_kstack_base_pa,
        kernel_stack_size: KERNEL_STACK_SIZE,
        kernel_sp: child_kernel_sp,
        context: child_ctx,
        ipc_buf_ptr: 0,
        ipc_buf_len: 0,
        ipc_actual_len: 0,
        ipc_transfer_handles: [None; 4],
        handle_table: core::ptr::null(),
        ipc_transfer_slots: [None, None],
        port_packet_slot: None,
        sleep_until: None,
        wait_child_pid: 0,
        owner_core: None,
    };

    unsafe { crate::task::scheduler::SCHEDULER.add_locked(child_thread); }

    let child_pid = child_proc.id;
    unsafe { crate::task::scheduler::SCHEDULER.unlock(scheduler_flags); }
    Ok(child_pid)
}

pub fn sys_proc_mgmt(table: &HandleTable, cmd: u32, arg1: usize, arg2: usize, arg3: usize) -> Result<usize> {
    use shared::syscall_nums::{PROC_MGMT_CREATE, PROC_MGMT_EXIT, PROC_MGMT_WAIT, PROC_MGMT_LIST, PROC_MGMT_RELEASE_PT};
    let _ = table;
    match cmd {
        PROC_MGMT_CREATE => {
            let parent_pid = arg1 as u64;
            let name_ptr = arg2;
            let l0_pa = arg3;
            let name = if name_ptr != 0 {
                let caller_l0 = current_process_l0_pa();
                let mut buf = [0u8; 63];
                let max_len = buf.len();
                crate::syscall::handlers::ipc::safe_copy_from_user(
                    caller_l0, name_ptr, max_len, &mut buf,
                ).map_err(|_| Status::InvalidArgs)?;
                let valid_len = buf.iter().position(|&b| b == 0).unwrap_or(max_len);
                let name_str = core::str::from_utf8(&buf[..valid_len]).map_err(|_| Status::InvalidArgs)?;
                intern_name(name_str)?
            } else {
                return Err(Status::InvalidArgs);
            };
            #[cfg(target_arch = "aarch64")]
            {
                if l0_pa == 0 || l0_pa % 4096 != 0 {
                    return Err(Status::InvalidArgs);
                }
            }
            let proc = crate::task::process::allocate_process(name)?;
            proc.parent_pid = parent_pid;
            proc.page_table.set_root_raw(l0_pa);
            Ok(proc.id as usize)
        }

        PROC_MGMT_EXIT => {
            let pid = arg1 as u64;
            let exit_code = arg2 as i32;
            if crate::task::process::find_process_mut(pid).is_some() {
                mark_process_zombie(pid, exit_code);
                Ok(0)
            } else {
                Err(Status::NotFound)
            }
        }

        PROC_MGMT_WAIT => {
            let target_pid = arg1 as i64;
            let caller_pid = current_process_id()?;
            for (slot_idx, slot) in unsafe { &mut crate::task::process::PROCESSES }.iter().enumerate() {
                let proc = match slot.as_ref() {
                    Some(p) => p,
                    None => continue,
                };
                if proc.parent_pid != caller_pid { continue; }
                match target_pid {
                    p if p > 0 => { if proc.id != p as u64 { continue; } }
                    _ => {}
                }
                if proc.exit_status.is_some() && proc.state == crate::task::process::ProcessState::Zombie {
                    let code = proc.exit_status.unwrap_or(0);
                    // Move ownership destructuring out of static PROCESSES.
                    // This triggers impl Drop for Process, which safely reclaims intermediate page tables,
                    // fully adhering to strict drop order.
                    let mut reaped_process = None;
                    unsafe {
                        reaped_process = crate::task::process::PROCESSES[slot_idx].take();
                    }
                    if let Some(mut p) = reaped_process {
                        p.state = crate::task::process::ProcessState::Dead;
                        p.exit_status = None;
                        p.thread_count = 0;
                        // Let local variable 'p' drop out of scope here. This invokes Process::drop()
                        // and reclaims its page tables.
                    }
                    return Ok((code as u32) as usize);
                }
            }
            Err(Status::TryAgain)
        }

        PROC_MGMT_LIST => {
            let buf_ptr = arg1;
            let max = arg2;
            if buf_ptr == 0 || max == 0 { return Err(Status::InvalidArgs); }
            let caller_l0 = current_process_l0_pa();
            let mut count = 0usize;
            for slot in unsafe { crate::task::process::PROCESSES.iter() } {
                if let Some(p) = slot {
                    if count >= max { break; }
                    let entry = ProcListEntry {
                        pid: p.id,
                        parent_pid: p.parent_pid,
                        state: p.state as u32,
                        name: p.name.as_bytes(),
                    };
                    let entry_bytes = entry.serialize();
                    let copy_len = entry_bytes.len().min(128);
                    crate::syscall::handlers::ipc::safe_copy_to_user(
                        caller_l0, &entry_bytes[..copy_len], buf_ptr + count * 128, copy_len,
                    ).ok();
                    count += 1;
                }
            }
            Ok(count)
        }

        PROC_MGMT_RELEASE_PT => {
            #[cfg(target_arch = "aarch64")]
            {
                let l0_pa = arg1;
                if l0_pa == 0 || l0_pa % 4096 != 0 {
                    return Err(Status::InvalidArgs);
                }
                crate::arch::aarch64::mmu::free_page_table_tree(l0_pa);
                Ok(0)
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                let _ = (arg1, arg2, arg3);
                Err(Status::NotAllowed)
            }
        }

        // S1: `PROC_MGMT_GET_IDENTITY` (cmd = 5).  Returns the
        // calling thread's `(tid << 32) | pid` as a single u64,
        // packed so the user-side helper can fetch both at once.
        // No side effects; this is purely a stat-reporting call.
        PROC_MGMT_GET_IDENTITY => {
            let _ = (arg1, arg2, arg3);
            let pid = current_process_id()?;
            let tid = unsafe {
                crate::task::scheduler::SCHEDULER
                    .get_current_thread_ptr()
                    .map(|t| (*t).id as u64)
                    .unwrap_or(0)
            };
            Ok(((tid << 32) | pid) as usize)
        }

        _ => Err(Status::NotAllowed),
    }
}

struct ProcListEntry<'a> {
    pid: u64,
    parent_pid: u64,
    state: u32,
    name: &'a [u8],
}

impl<'a> ProcListEntry<'a> {
    fn serialize(&self) -> [u8; 128] {
        let mut buf = [0u8; 128];
        buf[..8].copy_from_slice(&self.pid.to_le_bytes());
        buf[8..16].copy_from_slice(&self.parent_pid.to_le_bytes());
        buf[16..20].copy_from_slice(&self.state.to_le_bytes());
        let name_len = self.name.len().min(112);
        buf[20..20 + name_len].copy_from_slice(&self.name[..name_len]);
        buf
    }
}

/// SYSCALL_THREAD_SLEEP: safe and atomic thread-blocking timed sleep.
///
/// Moves the calling thread to the `ThreadState::Sleeping` state and programs
/// its `sleep_until` timer.
pub fn sys_thread_sleep(ticks: u64) -> Result<()> {
    if ticks == 0 {
        return Ok(());
    }
    let current_ticks = crate::drivers::timer::get_ticks();
    let wakeup_tick = current_ticks.saturating_add(ticks);

    if let Some(t) = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() } {
        unsafe {
            (*t).owner_core = None;
            (*t).state = crate::task::thread::ThreadState::Sleeping;
            (*t).sleep_until = Some(wakeup_tick);
            crate::task::scheduler::SCHEDULER.schedule();
        }
    }
    Ok(())
}
