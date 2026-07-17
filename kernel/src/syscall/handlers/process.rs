use crate::mm::vmar::VmarFlags;
use crate::mm::vmo::Vmo;
use crate::object::handle_table::{HandleTable, KernelObject};
use crate::object::rights::Rights;
use crate::task::process::{CWD_MAX, ProcessState};
use crate::task::thread::Thread;
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
    crate::log_info!("SYSCALL", "Process exited with code {}", code);

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
    crate::log_info!("SYSCALL", "SCHEDULER schedule caller_pid {}, pid {}", caller_pid, code);

    // `schedule()` either switched to another thread (and will not
    // return) or hit the all-dead halt path.  The `wfe` / `wfi`
    // fallback is purely defensive: if a future change to schedule()
    // ever makes it return when no thread is runnable, we want the
    // CPU to park instead of busy-looping.
    loop {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!("wfe");
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("wfi");
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
    let caller_l0_pa = caller_proc.page_table.l0_pa();
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
    let _pid = crate::task::process::Process::launch_user_program_with_argv(
        name_static,
        bytes,
        &arg_bufs[..argv_count],
        &arg_lens[..argv_count],
        argv_count,
        caller_pid,
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

/// SYSCALL_SPAWN: like `sys_exec` but **does not replace the caller**.
///
/// Reads `path_ptr`/`path_len` from the caller's user VA, resolves a
/// short name (e.g. `"devmgr"` → `"system/bin/devmgr"`) against the
/// embedded rootfs, materialises the OHLINK binary into a new EL0
/// process, and returns its pid via `x0`.  The caller's thread keeps
/// running with its original `state`, so this is the primitive boot
/// services (loader / init) use to chain — spawn companion services,
/// continue running, sync up via channel_registry lookups, *then*
/// `exec` the next bootstrap stage.
///
/// Because the new process is added with `Ready` state but the caller
/// is *not* demoted to `Dead`, the caller and the new process coexist
/// in the scheduler queue until the next timer tick switches between
/// them.  Returns the new pid as a `u64`; the syscall ABI marshals
/// that back into `x0` for the caller.
pub fn sys_spawn(
    table: &HandleTable,
    path_ptr: usize,
    path_len: usize,
    argv_ptr: usize,
    argv_count: usize,
) -> Result<u64> {
    if path_ptr == 0 || path_len == 0 {
        return Err(Status::InvalidArgs);
    }
    if argv_count > EXECVE_MAX_ARGS {
        return Err(Status::InvalidArgs);
    }

    // Translate the caller's user VA for path + (optional) argv.
    let caller_pid = current_process_id()?;
    let caller_proc = crate::task::process::find_process_mut(caller_pid)
        .ok_or(Status::NotFound)?;
    let caller_l0_pa = caller_proc.page_table.l0_pa();
    if caller_l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    let mut path_buf = [0u8; 256];
    let copy_path_len = core::cmp::min(path_len, path_buf.len());
    
    // Hardening translation verification for direct map access to eliminate TLB/Cache mismatch EL1 Data Aborts
    #[cfg(target_arch = "aarch64")]
    if let Some(resolved_pa) = crate::arch::aarch64::mmu::translate_user_va(caller_l0_pa, path_ptr) {
        let kva = crate::mm::mmu::pa_to_kernel_va(resolved_pa);
        // Evict/Clean user rodata page to Point of Coherency (PoC) to make sure main memory has correct values
        unsafe {
            core::arch::asm!("dc civac, {0}", in(reg) kva, options(nomem, nostack));
            core::arch::asm!("dsb ish", options(nomem, nostack));
            core::arch::asm!("isb", options(nomem, nostack));
        }
    }


    if let Err(e) = crate::syscall::handlers::ipc::safe_copy_from_user(
        caller_l0_pa,
        path_ptr,
        copy_path_len,
        &mut path_buf[..copy_path_len],
    ) {
        crate::kprintln!("[SPAWN ERROR] sys_spawn safe_copy_from_user of path failed: {:?}", e);
        return Err(e);
    }
    let program_name = core::str::from_utf8(&path_buf[..copy_path_len])
        .map_err(|_| Status::InvalidArgs)?;

    let mut arg_bufs: [[u8; 256]; EXECVE_MAX_ARGS] = [[0u8; 256]; EXECVE_MAX_ARGS];
    let mut arg_lens: [usize; EXECVE_MAX_ARGS] = [0usize; EXECVE_MAX_ARGS];
    let mut total_bytes: usize = 0;

    if argv_count > 0 {
        if argv_ptr == 0 {
            return Err(Status::InvalidArgs);
        }
        for i in 0..argv_count {
            let mut pair = [0u8; 16];
            let pair_off = argv_ptr + i * 16;
            if let Err(e) = crate::syscall::handlers::ipc::safe_copy_from_user(
                caller_l0_pa,
                pair_off,
                16,
                &mut pair,
            ) {
                crate::kprintln!("[SPAWN ERROR] failed to copy argv pair at {:#x}: {:?}", pair_off, e);
                return Err(e);
            }
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
                if let Err(e) = crate::syscall::handlers::ipc::safe_copy_from_user(
                    caller_l0_pa,
                    s_ptr,
                    s_len,
                    &mut arg_bufs[i][..s_len],
                ) {
                    crate::kprintln!("[SPAWN ERROR] failed to copy argv string at {:#x}: {:?}", s_ptr, e);
                    return Err(e);
                }
            }
            arg_lens[i] = s_len;
            total_bytes = total_bytes.saturating_add(s_len);
        }
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
        crate::log_error!("SPAWN", "Program {} not found in rootfs path: {}", program_name, path_str);
        Status::NotFound
    })?;

    let name_static: &'static str = intern_name(program_name)?;
    let pid = crate::task::process::Process::launch_user_program_with_argv(
        name_static,
        bytes,
        &arg_bufs[..argv_count],
        &arg_lens[..argv_count],
        argv_count,
        caller_pid,
    )?;

    // Recover the pid that was just assigned inside launch_user_program
    // so the caller can hold it if it wants to (and so we can return it).

    crate::log_info!(
        "SPAWN",
        "{} spawned at EL0 with argv[{}] (pid={}, path={})",
        program_name,
        argv_count,
        pid,
        path_str
    );

    Ok(pid)
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
    let ht_kva = if ht_raw < crate::mm::mmu::KERNEL_OFFSET {
        crate::mm::mmu::pa_to_kernel_va(ht_raw)
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
) -> Result<u64> {
    use crate::object::handle_table::KernelObject;

    crate::kprintln!("DEBUG KERNEL sys_load_binary enter: vmo_handle_raw={}", vmo_handle_raw);

    let vmo_hv = HandleValue::new(vmo_handle_raw);

    // Read the full OHLINK image out of the source VMO into a heap-backed
    // VMO. The kernel stack is only 16 KiB (KERNEL_STACK_PAGES=4) so a
    // large stack buffer would overflow and corrupt adjacent trap frames.
    // Instead we allocate a kernel VMO of the same size as the source
    // image, copy bytes into it via Vmo::read, then hand launch_user_program
    // a slice borrowed from the kernel heap mapping.
    let source_size_res = table.with_vmo(vmo_hv, Rights::READ.bits(), |vmo| -> usize {
        vmo.size()
    });

    let source_size = match source_size_res {
        Ok(sz) => {
            crate::kprintln!("DEBUG KERNEL sys_load_binary: source_size={}", sz);
            sz
        }
        Err(e) => {
            crate::kprintln!("DEBUG KERNEL sys_load_binary: table.with_vmo failed with {:?}", e);
            return Err(e);
        }
    };

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
    // buffer (LOAD_BINARY_SCRATCH, defined at module scope).  Reading directly
    // into the global scratch buffer slice completely avoids allocating large
    // 4 KiB buffers on the small, limited 16 KiB kernel stack, preventing stack corruption.
    let copied_into_scratch: usize = table.with_vmo(vmo_hv, Rights::READ.bits(), |src_vmo| -> usize {
        let mut total = 0usize;
        let scratch_cap = LOAD_BINARY_SCRATCH_SIZE;
        while total < src_vmo.size() && total < scratch_cap {
            let want = core::cmp::min(4096, core::cmp::min(src_vmo.size(), scratch_cap) - total);
            let dst_slice = unsafe {
                core::slice::from_raw_parts_mut(
                    LOAD_BINARY_SCRATCH.as_mut_ptr().add(total),
                    want
                )
            };
            if src_vmo.read(total, dst_slice).unwrap_or(0) == 0 {
                break;
            }
            total += want;
        }
        total
    })?;

    let bytes_slice: &[u8] = unsafe { &LOAD_BINARY_SCRATCH[..copied_into_scratch] };
    let name_static: &'static str = "user-prog";

    crate::kprintln!("DEBUG KERNEL sys_load_binary: assigned pid=NEW");

    let pid = crate::task::process::Process::launch_user_program(name_static, bytes_slice)?;
    crate::kprintln!("DEBUG KERNEL sys_load_binary: assigned pid={}", pid);

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
    crate::log_info!("SYSCALL_SPAWN", "sys_service_spawn: Entering ServiceLauncher::launch for desc: name={}, path={}", desc.name, desc.path);
    let pid = crate::loader::ServiceLauncher::launch(&desc)?;
    crate::log_info!("SYSCALL_SPAWN", "sys_service_spawn: ServiceLauncher::launch succeeded with PID={}", pid);
    
    // Register the Process handle so the caller can wait/terminate it normally
    let rights = Rights::READ.bits() | Rights::WRITE.bits();
    let _ = table.add(crate::object::handle_table::KernelObject::Process(pid), rights);
    
    crate::log_info!("SYSCALL_SPAWN", "sys_service_spawn: Successfully added to handle table, returning PID={}", pid);
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
    crate::log_info!(
        "CHDIR",
        "process '{}' cwd -> {:?}",
        proc.name,
        core::str::from_utf8(&proc.cwd[..proc.cwd_len]).unwrap_or("?")
    );
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
/// On success returns the pid of the reaped child.
pub fn sys_wait4(
    table: &HandleTable,
    pid: i64,
    status_out_ptr: usize,
    _options: i32,
) -> Result<u64> {
    let _ = table;

    let caller_pid = current_process_id()?;

    if pid < -1 {
        return Err(Status::InvalidArgs);
    }

    // Walk the process table for a direct child of the caller that
    // matches the requested pid filter.  The walk is O(N) but N is
    // bounded at MAX_PROCESSES = 8.
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
            found_reap_target = Some((slot_idx, proc.id, proc.exit_status.unwrap_or(0)));
            break;
        } else {
            // Track an alive direct child so we can tell the caller
            // "your child X is still alive" if no zombie is ready.
            found_running = Some(proc.id);
        }
    }

    if let Some((slot_idx, reaped_pid, code)) = found_reap_target {
        // Reap: clear exit_status, mark Dead, free the slot
        // index to be re-allocated.  We do NOT immediately free
        // the slot (`Some(Process::new)` placement); the slot
        // index is set to None so MAX_PROCESSES can grow back.
        if status_out_ptr != 0 {
            // Translate through caller's L0 (still the calling
            // thread's per-process page table; this function runs
            // in SVC handler context with the caller's TTBR0
            // live).  safe_copy_to_user handles null/length
            // checks internally.
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
        let _ = reaped_process; // Drop the reaped Process, triggering Process::drop()
        return Ok(reaped_pid);
    }

    if let Some(running_pid) = found_running {
        crate::log_info!(
            "WAIT4",
            "caller pid={} has running child pid={}, no zombie yet",
            caller_pid,
            running_pid
        );
        return Err(Status::TryAgain);
    }
    Err(Status::NotFound)
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
    // Spin-yield: until either a signal is delivered (Zombie)
    // or the caller changes its own disposition to ignore, we
    // call SCHEDULER.schedule() and let the timer tick wake us
    // back.  1.0 has no sync wakeup primitive on EL0 yet, so the
    // poll cadence is the scheduler tick (~10 ms) - good enough
    // for shell pipelines.
    let mut counter: u32 = 0;
    loop {
        let proc = crate::task::process::find_process_mut(caller_pid)
            .ok_or(Status::NotFound)?;
        if matches!(proc.state, ProcessState::Zombie) {
            // Signal has dispatched via SIG_DFL.
            return Ok(());
        }
        if proc.pending_signals == 0 {
            // Wait for one.
            counter = counter.wrapping_add(1);
            unsafe { crate::task::scheduler::SCHEDULER.schedule(); }
            // Avoid hot-spin
            if counter > 10000 {
                return Err(Status::TimedOut);
            }
            continue;
        }
        // Pending and not yet dispatched.  Force-dispatch now.
        if crate::task::signals::dispatch_pending(caller_pid)? {
            return Ok(());
        }
        // All bits were IGN.
        unsafe { crate::task::scheduler::SCHEDULER.schedule(); }
    }
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

    crate::log_info!(
        "PIPE",
        "allocated pipe id={} for pid={} -> read_fd={} write_fd={}",
        pipe_id.0,
        caller_pid,
        rd_fd,
        wr_fd
    );
    Ok(())
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
