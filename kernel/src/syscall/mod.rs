pub mod validation;
pub mod handlers;
pub mod capability;
pub mod lifecycle;

pub use validation::*;

use shared::status::Status;
use crate::object::handle_table::HandleTable;

/// Global handle table pointer, set once during kernel init.
pub static mut GLOBAL_HANDLE_TABLE: *const HandleTable = core::ptr::null();

pub fn set_handle_table(table: &HandleTable) {
    unsafe { GLOBAL_HANDLE_TABLE = table as *const HandleTable; }
}

fn handle_table() -> Option<&'static HandleTable> {
    unsafe { GLOBAL_HANDLE_TABLE.as_ref() }
}

pub fn syscall_dispatch(
    syscall_num: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> usize {
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let table = match thread_ptr {
        Some(t) if !unsafe { (*t).handle_table.is_null() } => unsafe { &*(*t).handle_table },
        _ => match handle_table() {
            Some(t) => t,
            None => return Status::NotAllowed.to_raw(),
        }
    };

    use shared::syscall_nums::*;
    // 🚀 特权拦截：早期自举调试专属的 UART 字符极速旁路！
    match syscall_num {
        SYSCALL_WRITE => {
            let fd = arg0;
            if fd == 1 || fd == 2 {
                return handlers::sys_write(fd, arg1, arg2);
            }
            // S2: pipe fds ≥ 3 dispatching through the kernel's
            // per-process fd_table (not the user-side
            // USER_FD_TABLE which only tracks Console/File/Pty
            // entries).  dispatch_pipe_io returns Ok(None) when
            // the fd is not a pipe, letting the caller fall
            // through to the generic NotAllowed path below.
            if fd as i32 >= 3 {
                match handlers::process::dispatch_pipe_io(
                    table, fd as u32, arg1, arg2, true,
                ) {
                    Ok(Some(n)) => return n,
                    Ok(None) => {}
                    Err(e) => return e.to_raw(),
                }
            }
        }
        SYSCALL_READ => {
            let fd = arg0;
            if fd == 0 {
                match handlers::vfs::sys_read(0, arg1, arg2) {
                    Ok(n) => return n,
                    Err(e) => return e.to_raw(),
                }
            }
            // S2: pipe read-end dispatching (mirrors the write
            // path above).  fd 0 is already handled by the
            // `handlers::vfs::sys_read` above; fds ≥ 3 are
            // checked against the per-process Pipe fd_table.
            if fd as i32 >= 3 {
                match handlers::process::dispatch_pipe_io(
                    table, fd as u32, arg1, arg2, false,
                ) {
                    Ok(Some(n)) => return n,
                    Ok(None) => {}
                    Err(e) => return e.to_raw(),
                }
            }
        }
        _ => {}
    }

    if let Some(res) = capability::dispatch_capability(table, syscall_num, arg0, arg1, arg2, arg3, arg4, arg5) {
        return res;
    }

    // 2. 其次尝试由特权级 Lifecycle 系统调用派发器匹配并处理
    if let Some(res) = lifecycle::dispatch_lifecycle(table, syscall_num, arg0, arg1, arg2, arg3, arg4, arg5) {
        return res;
    }

    // 3. Device info query (no handle table needed, read-only)
    if syscall_num == SYSCALL_DEVICE_INFO {
        return match handlers::device::sys_device_info(arg0, arg1) {
            Ok(n) => n,
            Err(e) => e.to_raw(),
        };
    }

    if syscall_num == SYSCALL_BLOCK_READ {
        return match handlers::device::sys_block_read(arg0 as u64, arg1) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_BLOCK_WRITE {
        return match handlers::device::sys_block_write(arg0 as u64, arg1) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_BLOCK_SIZE {
        return match handlers::device::sys_block_size() {
            Ok(size) => size as usize,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_NET_SEND {
        return match handlers::device::sys_net_send(arg0, arg1) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_NET_RECV {
        return match handlers::device::sys_net_recv(arg0, arg1) {
            Ok(n) => n,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_DISPLAY_FLUSH {
        return match handlers::device::sys_display_flush(table, arg0 as u32) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        };
    }

    // 3c. S1 identity / clock syscalls.
    if let Some(res) = dispatch_identity(syscall_num, arg0, arg1, arg2, arg3, arg4, arg5) {
        return res;
    }

    // 3d. S3 `fork()` — returns the child's pid in `x0` for the
    // parent; the child's return path carries `x0 = 0` because
    // `sys_fork` itself patches the cloned Aarch64Context.
    if syscall_num == shared::syscall_nums::SYSCALL_FORK {
        return match handlers::process::sys_fork() {
            Ok(pid) => pid as usize,
            Err(e) => e.to_raw(),
        };
    }

    // 3e. S6 `open("/dev/ptmx")` / `/dev/pts/N`.  Once userspace
    // has a TTY fd the rest of its TTY traffic stays on
    // `SYSCALL_PTY_READ` / `SYSCALL_PTY_WRITE` so the VFS
    // dispatcher doesn't have to know about PTY paths.
    if syscall_num == shared::syscall_nums::SYSCALL_TTY_OPEN {
        return match handlers::tty::sys_tty_open(arg0, arg1, arg2 as u32) {
            Ok(fd) => fd as usize,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == shared::syscall_nums::SYSCALL_PTY_READ {
        return match handlers::tty::sys_tty_read(arg0 as u32, arg1, arg2) {
            Ok(n) => n,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == shared::syscall_nums::SYSCALL_PTY_WRITE {
        return match handlers::tty::sys_tty_write(arg0 as u32, arg1, arg2) {
            Ok(n) => n,
            Err(e) => e.to_raw(),
        };
    }

    // 3f. S6 `ioctl(fd, req, arg)` dispatch.  TTY class ops
    // (TIOCGWINSZ / TCGETS / TCSETS / TIOCSCTTY / etc.) are
    // routed to `sys_tty_ioctl`; everything else returns
    // `Status::NotAllowed` until S7 / S9 land their
    // dedicated families (TIOCOND / SIOCGIFNAME / etc.).
    if syscall_num == shared::syscall_nums::SYSCALL_IOCTL {
        return match handlers::tty::sys_tty_ioctl(arg0 as u32, arg1 as u32, arg2) {
            Ok(ret) => ret as usize,
            Err(e) => e.to_raw(),
        };
    }

    // 3b. Privileged MMIO access (validated against known device bases)
    if syscall_num == SYSCALL_MMIO_READ {
        return match handlers::mmio::sys_mmio_read(arg0, arg1) {
            Ok(v) => v as usize,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_MMIO_WRITE {
        return match handlers::mmio::sys_mmio_write(arg0, arg1, arg2 as u32) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        };
    }

    // 3g. virtio-mmio bus primitives (microkernel principle: protocol
    // drivers live in EL0; the kernel only exposes the bus).
    if syscall_num == SYSCALL_VIRTIO_PROBE {
        return match crate::drivers::bus::mmio_bus::sys_virtio_probe(arg0, arg1) {
            Ok(n) => n,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_VIRTIO_SETUP_QUEUE {
        return match crate::drivers::bus::mmio_bus::sys_virtio_setup_queue(
            arg1 as u32, arg2 as u16, arg3 as u16,
        ) {
            Ok(handles) => {
                crate::log_info!(
                    "VIRTIO-BUS",
                    "sys_virtio_setup_queue marshal: desc_vmo={}, avail_vmo={}, used_vmo={}, desc_bytes={}, avail_bytes={}, used_bytes={}",
                    handles.desc_vmo, handles.avail_vmo, handles.used_vmo,
                    handles.desc_bytes, handles.avail_bytes, handles.used_bytes
                );
                // Marshal the 40-byte `VirtioQueueHandles` back to the caller.
                // We assume the user buffer is at least 40 bytes wide.
                let caller_pid = match crate::task::process::current_process_id() {
                    Ok(p) => p,
                    Err(_) => return Status::NotFound.to_raw(),
                };
                let l0_pa = match crate::task::process::find_process_mut(caller_pid) {
                    Some(p) => p.page_table.l0_pa(),
                    None => return Status::NotFound.to_raw(),
                };
                if l0_pa == 0 {
                    return Status::InvalidArgs.to_raw();
                }
                let raw = unsafe {
                    let dst = arg0 as *mut u8;
                    let src = &handles as *const _ as *const u8;
                    core::ptr::copy_nonoverlapping(src, dst, 56);
                };
                let _ = raw;
                56usize
            }
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_VIRTIO_KICK {
        return match crate::drivers::bus::mmio_bus::sys_virtio_kick(arg0 as u32, arg1 as u16) {
            Ok(()) => 0,
            Err(e) => e.to_raw(),
        };
    }
    if syscall_num == SYSCALL_VIRTIO_READ_ISR {
        return match crate::drivers::bus::mmio_bus::sys_virtio_read_isr(arg0 as u32) {
            Ok(v) => v as usize,
            Err(e) => e.to_raw(),
        };
    }

    // 4. 均未匹配，返回不受支持
    Status::NotAllowed.to_raw()
}

// -------------------------------------------------------------------------
// S1: Identity & clock dispatch.
//
// All of these return a single `usize` payload so the
// syscall_dispatch trampoline can keep its `usize` return-type
// contract.  Negative payloads are reserved for future Status
// translation if we ever wire more elaborate error reporting.
// -------------------------------------------------------------------------

#[inline]
fn identity_unary(zero_or_pid: usize) -> usize {
    // All current identity syscalls hard-code `uid = gid = 0`
    // and `ppid = 0` for the very first process.  For child
    // processes the real ppid is recovered via the scheduler's
    // per-thread `pid`.
    let _ = zero_or_pid;
    0
}

#[inline]
fn read_current_pid() -> usize {
    // SAFETY: the scheduler's `get_current_thread_ptr` already
    // either returns `Some` or is treated as "no current thread"
    // by callers.  For S1 we only need an integer, so the
    // unwrap_or(0) is acceptable.
    unsafe {
        let p = crate::task::scheduler::SCHEDULER.get_current_thread_ptr();
        p.map(|t| (*t).process_id as usize).unwrap_or(0)
    }
}

#[inline]
fn read_current_tid() -> usize {
    unsafe {
        let p = crate::task::scheduler::SCHEDULER.get_current_thread_ptr();
        p.map(|t| (*t).id).unwrap_or(0)
    }
}

/// POSIX `getuid()` — real uid of the caller.  Always 0.
pub fn sys_getuid(_arg0: usize) -> usize { 0 }
/// POSIX `geteuid()` — effective uid.  Always 0.
pub fn sys_geteuid(_arg0: usize) -> usize { 0 }
/// POSIX `getgid()` — real gid.  Always 0.
pub fn sys_getgid(_arg0: usize) -> usize { 0 }
/// POSIX `getegid()` — effective gid.  Always 0.
pub fn sys_getegid(_arg0: usize) -> usize { 0 }
/// POSIX `getppid()` — parent pid; 0 for the boot anchor.
pub fn sys_getppid(_arg0: usize) -> usize { 0 }
/// POSIX `getpgrp()` — current process group id.  In 1.0 we
/// model process groups as a flat u64 id stored on
/// `Process.pgroup`; getpgrp() reads it back.
pub fn sys_getpgrp(_arg0: usize) -> usize {
    if let Ok(pid) = crate::task::process::current_process_id() {
        if let Some(proc) = crate::task::process::find_process_mut(pid) {
            return proc.pgroup as usize;
        }
    }
    read_current_pid()
}
/// POSIX `getsid(pid)` — returns the caller's session id.
pub fn sys_getsid(_arg0: usize) -> usize {
    if let Ok(pid) = crate::task::process::current_process_id() {
        if let Some(proc) = crate::task::process::find_process_mut(pid) {
            return proc.sid as usize;
        }
    }
    read_current_pid()
}
/// POSIX `setsid()` — start a new session.  Returns the leader
/// pid (= the caller's own pid).  We model sessions as flat
/// u64 ids; "new session" is just "set sid = self".
pub fn sys_setsid(_arg0: usize) -> usize {
    if let Ok(pid) = crate::task::process::current_process_id() {
        if let Some(proc) = crate::task::process::find_process_mut(pid) {
            proc.sid = pid;
            return pid as usize;
        }
    }
    read_current_pid()
}
/// POSIX `setpgid(pid, pgrp)` — move the (caller's) process
/// into a new process group.  `pid=0` means the caller,
/// `pgrp=0` means "use the caller's pid as the new group".
/// 1.0 only supports those two convenience forms; cross-pid
/// pgroup moves are accepted but not enforced (every process
/// is its own pgroup in the flat model).
pub fn sys_setpgid(pid_arg: usize, pgrp_arg: usize) -> usize {
    let caller_pid = match crate::task::process::current_process_id() {
        Ok(p) => p,
        Err(_) => return 0,
    };
    // pid == 0 means caller; everything else is "best effort" —
    // we accept the call but only mutate the caller's own
    // pgroup, which is the only pgroup the caller can
    // legitimately move.
    if pid_arg != 0 {
        return caller_pid as usize;
    }
    let new_pgroup = if pgrp_arg == 0 {
        caller_pid
    } else {
        pgrp_arg as u64
    };
    if let Some(proc) = crate::task::process::find_process_mut(caller_pid) {
        proc.pgroup = new_pgroup;
    }
    new_pgroup as usize
}

/// POSIX `gettid()` — returns the kernel thread id of the caller.
pub fn sys_gettid() -> usize { read_current_tid() }

/// POSIX `gettimeofday(tv, tz)` — writes `{ tv_sec, tv_usec }`
/// at the user VA.  Returns 0 on success, `NotAllowed` on a bad
/// pointer.  `tz` (the timezone struct) is accepted but ignored;
/// POSIX allows programs to leave it NULL or a stale pointer and
/// we honour that.
///
/// The user VA is translated through the caller's L0 page
/// table by `safe_copy_to_user` (DEVELOPMENT.md §3: never
/// dereference a raw EL0 pointer at the syscall boundary).
pub fn sys_gettimeofday(tv_ptr: usize, _tz_ptr: usize) -> usize {
    if tv_ptr == 0 {
        return Status::NotAllowed.to_raw();
    }
    // Look up the caller's L0 PA so the safe-copy can translate
    // the user VA; refuse if the caller isn't wired up.
    let caller_pid = match crate::task::process::current_process_id() {
        Ok(p) => p,
        Err(_) => return Status::NotFound.to_raw(),
    };
    let l0_pa = match crate::task::process::find_process_mut(caller_pid) {
        Some(p) => p.page_table.l0_pa(),
        None => return Status::NotFound.to_raw(),
    };
    if l0_pa == 0 {
        return Status::InvalidArgs.to_raw();
    }

    // Read the physical counter (CNTPCT_EL0 on aarch64).  We use
    // the same counter exposed by `drivers::timer::phys_count()`.
    let ticks = crate::drivers::timer::phys_count();
    let freq = crate::drivers::timer::freq_hz() as u64;
    // freq is bounded by QEMU's 62.5 MHz default, but we don't
    // hand it out — return seconds + microseconds.
    let us = ticks.saturating_mul(1_000_000) / freq.max(1);
    let sec = (us / 1_000_000) as i64;
    let us_part = (us % 1_000_000) as i64;
    let layout = [sec.to_le(), us_part.to_le()];

    let bytes = unsafe {
        core::slice::from_raw_parts(
            layout.as_ptr() as *const u8,
            core::mem::size_of::<[i64; 2]>(),
        )
    };
    let write_len = bytes.len();
    match crate::syscall::handlers::ipc::safe_copy_to_user(
        l0_pa,
        bytes,
        tv_ptr,
        write_len,
    ) {
        Ok(()) => 0,
        Err(e) => e.to_raw(),
    }
}

/// Internal: dispatch for the S1 identity / clock class.
pub fn dispatch_identity(
    syscall_num: u32,
    arg0: usize,
    arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    _arg5: usize,
) -> Option<usize> {
    use shared::syscall_nums::*;
    let r = match syscall_num {
        SYSCALL_GETUID => sys_getuid(arg0),
        SYSCALL_GETEUID => sys_geteuid(arg0),
        SYSCALL_GETGID => sys_getgid(arg0),
        SYSCALL_GETEGID => sys_getegid(arg0),
        SYSCALL_GETPPID => sys_getppid(arg0),
        SYSCALL_GETPGRP => sys_getpgrp(arg0),
        SYSCALL_GETSID => sys_getsid(arg0),
        SYSCALL_SETSID => sys_setsid(arg0),
        SYSCALL_SETPGID => sys_setpgid(arg0, arg1),
        SYSCALL_GETTIMEOFDAY => sys_gettimeofday(arg0, arg1),
        _ => return None,
    };
    Some(r)
}
