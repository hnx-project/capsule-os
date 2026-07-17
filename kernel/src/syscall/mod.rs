pub mod validation;
pub mod handlers;
pub mod capability;
pub mod lifecycle;

pub use validation::*;

use shared::status::Status;
use crate::object::handle_table::HandleTable;

/// Global handle table pointer, set once during kernel init.
static mut GLOBAL_HANDLE_TABLE: *const HandleTable = core::ptr::null();

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
        }
        SYSCALL_READ => {
            let fd = arg0;
            if fd == 0 {
                match handlers::vfs::sys_read(0, arg1, arg2) {
                    Ok(n) => return n,
                    Err(e) => return e.to_raw(),
                }
            }
        }
        _ => {}
    }

    // 1. 优先尝试由一等公民 Capability 系统调用派发器匹配并处理
    if let Some(res) = capability::dispatch_capability(table, syscall_num, arg0, arg1, arg2, arg3, arg4, arg5) {
        return res;
    }

    // 2. 其次尝试由特权级 Lifecycle 系统调用派发器匹配并处理
    if let Some(res) = lifecycle::dispatch_lifecycle(table, syscall_num, arg0, arg1, arg2, arg3, arg4, arg5) {
        return res;
    }

    // 3. 均未匹配，返回不受支持
    Status::NotAllowed.to_raw()
}
