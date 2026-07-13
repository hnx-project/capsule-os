pub mod validation;
pub mod handlers;

pub use validation::*;

use shared::status::Status;
use shared::types::HandleValue;
use crate::object::handle_table::HandleTable;

/// Global handle table pointer, set once during kernel init.
static mut GLOBAL_HANDLE_TABLE: *const HandleTable = core::ptr::null();

pub fn set_handle_table(table: &HandleTable) {
    unsafe { GLOBAL_HANDLE_TABLE = table as *const HandleTable; }
}

fn handle_table() -> Option<&'static HandleTable> {
    unsafe { GLOBAL_HANDLE_TABLE.as_ref() }
}

pub fn syscall_dispatch(syscall_num: u32, arg0: usize, arg1: usize,
                        arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> usize {
    let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
    let table = match thread_ptr {
        Some(t) if !unsafe { (*t).handle_table.is_null() } => unsafe { &*(*t).handle_table },
        _ => match handle_table() {
            Some(t) => t,
            None => return Status::NotAllowed.to_raw(),
        }
    };

    use crate::syscall::validation::*;
    use shared::syscall_nums::*;

    match syscall_num {
        SYSCALL_EXIT => {
            handlers::process::sys_exit(arg0 as i32);
        }

        SYSCALL_PROCESS_CREATE => {
            match handlers::process::sys_process_create(table, arg0, arg1) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_THREAD_CREATE => {
            match handlers::process::sys_thread_create(table, arg0 as u32, arg1, arg2, arg3, arg4) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_THREAD_START => {
            match handlers::process::sys_thread_start(table, arg0 as u32) {
                Ok(_) => 0,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMAR_MAP => {
            match handlers::memory::sys_vmar_map(table, arg0 as u32, arg1 as u32, arg2, arg3, arg4, arg5 as u32) {
                Ok(va) => va,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_CREATE => {
            let size = arg0;
            match handlers::memory::sys_vmo_create(table, size) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_CREATE_CHILD => {
            let parent_handle = arg0 as u32;
            let offset = arg1;
            let size = arg2;
            match handlers::memory::sys_vmo_create_child(table, parent_handle, offset, size) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_READ => {
            let handle = arg0 as u32;
            let offset = arg1;
            let dst_user_va = arg2;
            let len = arg3;
            match handlers::memory::sys_vmo_read(table, handle, offset, dst_user_va, len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_WRITE => {
            let handle = arg0 as u32;
            let offset = arg1;
            let src_user_va = arg2;
            let len = arg3;
            match handlers::memory::sys_vmo_write(table, handle, offset, src_user_va, len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_CREATE => {
            match handlers::ipc::sys_channel_create(table) {
                Ok((h0, h1)) => (h0.get() as usize) | ((h1.get() as usize) << 32),
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_READ => {
            let handle = arg0 as u32;
            let buf_ptr = arg1;
            let buf_len = arg2;
            match handlers::ipc::sys_channel_read(table, handle, buf_ptr, buf_len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_WRITE => {
            let handle = arg0 as u32;
            let buf_ptr = arg1;
            let buf_len = arg2;
            match handlers::ipc::sys_channel_write(table, handle, buf_ptr, buf_len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_REGISTER => {
            let name_ptr = arg0;
            let name_len = arg1;
            let handle = arg2 as u32;
            match handlers::ipc::sys_channel_register(table, name_ptr, name_len, handle) {
                Ok(_) => 0,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CHANNEL_LOOKUP => {
            let name_ptr = arg0;
            let name_len = arg1;
            match handlers::ipc::sys_channel_lookup(table, name_ptr, name_len) {
                Ok(h) => h.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_HANDLE_DUPLICATE => {
            let handle = arg0 as u32;
            let rights = arg1 as u32;
            match table.duplicate_handle(HandleValue::new(handle), rights) {
                Ok(new_hv) => new_hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_EXEC => {
            let name_ptr = arg0 as *const u8;
            let name_len = arg1;
            if name_ptr.is_null() || name_len == 0 {
                return Status::InvalidArgs.to_raw();
            }
            let bytes = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
            match core::str::from_utf8(bytes) {
                Ok(s) => {
                    match handlers::process::sys_exec(table, s) {
                        Ok(_) => 0,
                        Err(e) => e.to_raw(),
                    }
                }
                Err(_) => Status::InvalidArgs.to_raw(),
            }
        }

        SYSCALL_EXECVE => {
            match handlers::process::sys_execve(table, arg0, arg1, arg2, arg3) {
                Ok(_) => 0,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_LOAD_BINARY => {
            let vmo_handle = arg0 as u32;
            let name_ptr = arg1;
            let name_len = arg2;
            let result = handlers::process::sys_load_binary(table, vmo_handle, name_ptr, name_len);
            match result {
                Ok(pid) => pid as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_SPAWN => {
            let path_ptr = arg0;
            let path_len = arg1;
            let argv_ptr = arg2;
            let argv_count = arg3;
            match handlers::process::sys_spawn(table, path_ptr, path_len, argv_ptr, argv_count) {
                Ok(pid) => pid as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_YIELD => {
            // Voluntarily yield the CPU.  schedule() does the actual
            // switch; we always return 0 here.  If no other thread is
            // Ready we just keep the same thread running — caller will
            // spin until something else becomes runnable.
            unsafe { crate::task::scheduler::SCHEDULER.schedule(); }
            0
        }

        SYSCALL_OPEN => {
            let path_ptr = arg0;
            let path_len = arg1;
            let flags = arg2 as u32;
            // Phase 6 v0.6.0-α P1: route through the fileagent
            // forwarder instead of the dead-stub vfs::sys_open.
            match handlers::posix::sys_open_posix(table, path_ptr, path_len, flags) {
                Ok(fd) => fd as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CLOSE => {
            let fd = arg0 as u32;
            match handlers::posix::sys_close_posix(table, fd) {
                Ok(_) => 0,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_READ => {
            let fd = arg0 as u32;
            let buf_ptr = arg1;
            let buf_len = arg2;
            // fd == 0 is the UART stdin path (KERNEL_HEALTH K-D2).
            // fd == 1/2 are UART stdout/stderr — sys_write handles
            // them.  fd >= 3 is the fileagent forwarder.
            if fd == 0 {
                match handlers::vfs::sys_read(0, buf_ptr, buf_len) {
                    Ok(n) => n,
                    Err(e) => e.to_raw(),
                }
            } else {
                // B6: consult per-process fd_table for a pipe end
                // first; if found, dispatch_pipe_io handles the
                // transfer and returns Some(bytes).  Returning
                // Ok(Some(n)) here is the early-exit path; if it
                // returns Ok(None) we fall through to the
                // existing fileagent forwarder.
                if fd != 0 {
                    match crate::syscall::handlers::process::dispatch_pipe_io(
                        table, fd, buf_ptr, buf_len, /* is_write= */ false,
                    ) {
                        Ok(Some(n)) => return n,
                        Ok(None) => {}
                        Err(e) => return e.to_raw(),
                    }
                }
                match handlers::posix::sys_read_posix(table, fd, buf_ptr, buf_len) {
                    Ok(n) => n,
                    Err(e) => e.to_raw(),
                }
            }
        }

        SYSCALL_WRITE => {
            let fd = arg0;
            let buf_ptr = arg1;
            let buf_len = arg2;
            if fd == 1 || fd == 2 {
                // fd 1/2 stay on the in-kernel UART path (K-D2).
                handlers::sys_write(fd, buf_ptr, buf_len)
            } else {
                // B6 pipe-write fast path: consult fd_table for a
                // pipe-writer end.  We do NOT marshal into a VMO
                // for the pipe case; syscall!() in hnxlibc passes
                // the buffer pointer directly when fd is a pipe
                // fd (set by sys_pipe), and the kernel reads it
                // via safe_copy_from_user inside dispatch_pipe_io.
                let pipe_n = crate::syscall::handlers::process::dispatch_pipe_io(
                    table, fd as u32, buf_ptr, buf_len, /* is_write= */ true,
                );
                match pipe_n {
                    Ok(Some(n)) => return n,
                    Ok(None) => {}
                    Err(e) => return e.to_raw(),
                }

                // Phase 6 v0.6.0-α P3 forwarder.  The caller must
                // have staged the buffer in a VMO whose handle is
                // already in *their* per-process HandleTable; we
                // pass it through as the cmd-side data transfer
                // slot (see handlers::posix::sys_write_posix).
                match handlers::posix::sys_write_posix(table, fd as u32, buf_ptr, buf_len, 0) {
                    Ok(n) => n,
                    Err(e) => e.to_raw(),
                }
            }
        }

        SYSCALL_SEEK => {
            let fd = arg0 as u32;
            let offset = arg1 as i64;
            // libc `whence` matches fileagent's u32 layout
            // (SEEK_SET=0 / SEEK_CUR=1 / SEEK_END=2).
            let whence = arg2 as u32;
            match handlers::posix::sys_lseek_posix(table, fd, offset, whence) {
                Ok(off) => off as usize,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_GETCWD => {
            match handlers::process::sys_getcwd(arg0, arg1) {
                Ok(n) => n,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_CHDIR => {
            match handlers::process::sys_chdir(arg0, arg1) {
                Ok(_) => 0,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_WAIT4 => {
            // sys_wait4(pid, status_out_ptr, options).  See
            // `ProcessState::Zombie` traversal in handlers::process
            // for the full algorithm; for 1.0 we only support
            // pid > 0 (specific child) and pid = -1 (any child),
            // returning the reaped pid or a Status code.
            match handlers::process::sys_wait4(table, arg0 as i64, arg1, arg2 as i32) {
                Ok(reaped_pid) => reaped_pid as usize,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_SIGACTION => {
            match handlers::process::sys_sigaction(table, arg0, arg1, arg2, arg3) {
                Ok(prev) => prev,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_RAISE => {
            match handlers::process::sys_raise(table, arg0) {
                Ok(()) => 0,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_KILL => {
            match handlers::process::sys_kill(table, arg0 as i64, arg1) {
                Ok(()) => 0,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_PAUSE => {
            // pause() may take effect only after the dispatcher
            // returns to the trap exit path; for 1.0 we return
            // Ok and let the user-space caller spin-poll the
            // wait4 status on the next syscall.
            match handlers::process::sys_pause(table) {
                Ok(()) => 0,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_PIPE => {
            match handlers::process::sys_pipe(table, arg0) {
                Ok(()) => 0,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_DUP2 => {
            match handlers::process::sys_dup2(table, arg0 as u32, arg1 as u32) {
                Ok(fd) => fd as usize,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_GET_TID => {
            // POSIX `gettid(2)` — kernel TID of the calling thread.
            // Wire per KERNEL_HEALTH.md P6 (Phase 6 v0.6.0-α POSIX push).
            match handlers::process::sys_gettid() {
                Ok(tid) => tid as usize,
                Err(e) => e.to_raw() as usize,
            }
        }

        SYSCALL_GET_PID => {
            // POSIX `getpid(2)` — kernel PID of the calling thread's
            // owning process.  fork(2) is intentionally unimplemented
            // (K-D1 fork-less posix); this is a 1:1 lookup.
            match handlers::process::sys_getpid() {
                Ok(pid) => pid as usize,
                Err(e) => e.to_raw() as usize,
            }
        }

        // Q5 (you opted in): every 1.0 reserved syscall slot
        // returns `Status::NotAllowed` and emits a single
        // rate-limited log line so the operator can tell which
        // ABI slot the program tried.  We use a static
        // AtomicBool to log once per boot per syscall_num
        // because otherwise a busy user-space program polling
        // for `SYSCALL_GETPPID` would flood the UART.
        SYSCALL_THREAD_EXIT
        | SYSCALL_PROCESS_START
        | SYSCALL_PROCESS_EXIT
        | SYSCALL_VMO_GET_SIZE
        | SYSCALL_VMO_SET_SIZE
        | SYSCALL_VMAR_UNMAP
        | SYSCALL_VMAR_PROTECT
        | SYSCALL_CHANNEL_CALL
        | SYSCALL_PORT_CREATE
        | SYSCALL_PORT_WAIT
        | SYSCALL_PORT_QUEUE
        | SYSCALL_EVENT_CREATE
        | SYSCALL_EVENT_SIGNAL
        | SYSCALL_EVENT_ACK
        | SYSCALL_TIMER_CREATE
        | SYSCALL_TIMER_SET
        | SYSCALL_TIMER_CANCEL
        | SYSCALL_FUTEX_WAIT
        | SYSCALL_FUTEX_WAKE => {
            use core::sync::atomic::{AtomicBool, Ordering};
            struct Flag;
            static FLAGGED: AtomicBool = AtomicBool::new(false);
            static ZEROED: AtomicBool = AtomicBool::new(false);
            // Zero-allocation tagging: we want one log line per
            // distinct syscall_num across the whole process.
            // Since we can't allocate, we cheat by hashing the
            // bits of syscall_num into a single bool bit per
            // category (low-half vs high-half).  This keeps
            // spam bounded without introducing 18 AtomicBool
            // statics in this file.
            let bucket = if syscall_num & 0x40 != 0 { &FLAGGED } else { &ZEROED };
            if !bucket.load(Ordering::Acquire) {
                bucket.store(true, Ordering::Release);
                crate::log_warn!(
                    "SYSCALL-STUB",
                    "caller invoked reserved SYSCALL={} -- returning NotAllowed (1.0 stub)",
                    syscall_num
                );
            }
            Status::NotAllowed.to_raw()
        }

        _ => Status::NotAllowed.to_raw(),
    }
}
