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

        SYSCALL_VMO_READ => {
            let handle = arg0 as u32;
            let offset = arg1;
            let dst = arg2 as *mut u8;
            let len = core::cmp::min(arg3, 64);
            if dst.is_null() || len == 0 {
                return Status::InvalidArgs.to_raw();
            }
            let mut buf = [0u8; 64];
            let slice = &mut buf[..len];
            match handlers::memory::sys_vmo_read(table, handle, offset, slice) {
                Ok(n) => {
                    unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, n); }
                    n
                }
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_VMO_WRITE => {
            let handle = arg0 as u32;
            let offset = arg1;
            let src = arg2 as *const u8;
            let len = core::cmp::min(arg3, 64);
            let buf = if !src.is_null() && len > 0 {
                unsafe { core::slice::from_raw_parts(src, len) }
            } else {
                &[]
            };
            match handlers::memory::sys_vmo_write(table, handle, offset, buf) {
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

        _ => Status::NotAllowed.to_raw(),
    }
}
