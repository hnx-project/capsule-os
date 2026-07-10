pub mod numbers;
pub mod validation;
pub mod handlers;

pub use numbers::*;
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

    use crate::syscall::numbers::*;

    match syscall_num {
        SYSCALL_EXIT => {
            handlers::process::sys_exit(arg0 as i32);
        }

        SYSCALL_WRITE => {
            handlers::sys_write(arg0, arg1, arg2)
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

        SYSCALL_OPEN => {
            let path_ptr = arg0;
            let path_len = arg1;
            let flags = arg2 as u32;
            match handlers::vfs::sys_open(path_ptr, path_len, flags) {
                Ok(fd) => fd as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_CLOSE => {
            let fd = arg0 as u32;
            match handlers::vfs::sys_close(fd) {
                Ok(_) => 0,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_READ => {
            let fd = arg0 as u32;
            let buf_ptr = arg1;
            let buf_len = arg2;
            match handlers::vfs::sys_read(fd, buf_ptr, buf_len) {
                Ok(n) => n,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_SEEK => {
            match handlers::vfs::sys_seek(arg0 as u32, arg1 as i64, arg2 as i32) {
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

        _ => Status::NotAllowed.to_raw(),
    }
}
