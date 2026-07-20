use crate::object::handle_table::HandleTable;
use crate::syscall::handlers;
use shared::status::Status;

pub fn dispatch_lifecycle(
    table: &HandleTable,
    syscall_num: u32,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> Option<usize> {
    use shared::syscall_nums::*;

    let res = match syscall_num {
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
            match handlers::process::sys_thread_create(
                table,
                arg0 as u32,
                arg1,
                arg2,
                arg3,
                arg4,
            ) {
                Ok(hv) => hv.get() as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_THREAD_START => match handlers::process::sys_thread_start(table, arg0 as u32) {
            Ok(_) => 0,
            Err(e) => e.to_raw(),
        },

        SYSCALL_EXEC => {
            let name_ptr = arg0 as *const u8;
            let name_len = arg1;
            if name_ptr.is_null() || name_len == 0 {
                return Some(Status::InvalidArgs.to_raw());
            }
            let bytes = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
            match core::str::from_utf8(bytes) {
                Ok(s) => match handlers::process::sys_exec(table, s) {
                    Ok(_) => 0,
                    Err(e) => e.to_raw(),
                },
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
            let vmo_offset = arg3;
            let result = handlers::process::sys_load_binary(table, vmo_handle, name_ptr, name_len, vmo_offset);
            match result {
                Ok(pid) => pid as usize,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_SERVICE_SPAWN => {
            let desc_ptr = arg0;
            let result = handlers::process::sys_service_spawn(table, desc_ptr);
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
            unsafe {
                crate::task::scheduler::SCHEDULER.schedule();
            }
            0
        }

        SYSCALL_PROC_MGMT => {
            let cmd = arg0 as u32;
            match handlers::process::sys_proc_mgmt(table, cmd, arg1, arg2, arg3) {
                Ok(v) => v,
                Err(e) => e.to_raw(),
            }
        }

        SYSCALL_GETCWD => match handlers::process::sys_getcwd(arg0, arg1) {
            Ok(n) => n,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_CHDIR => match handlers::process::sys_chdir(arg0, arg1) {
            Ok(_) => 0,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_WAIT4 => {
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

        SYSCALL_RAISE => match handlers::process::sys_raise(table, arg0) {
            Ok(()) => 0,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_KILL => match handlers::process::sys_kill(table, arg0 as i64, arg1) {
            Ok(()) => 0,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_PAUSE => match handlers::process::sys_pause(table) {
            Ok(()) => 0,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_PIPE => match handlers::process::sys_pipe(table, arg0) {
            Ok(()) => 0,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_DUP2 => match handlers::process::sys_dup2(table, arg0 as u32, arg1 as u32) {
            Ok(fd) => fd as usize,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_GET_TID => match handlers::process::sys_gettid() {
            Ok(tid) => tid as usize,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_GET_PID => match handlers::process::sys_getpid() {
            Ok(pid) => pid as usize,
            Err(e) => e.to_raw() as usize,
        },

        SYSCALL_THREAD_EXIT
        | SYSCALL_PROCESS_START
        | SYSCALL_PROCESS_EXIT
        | SYSCALL_VMO_GET_SIZE
        | SYSCALL_VMO_SET_SIZE
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

        _ => return None,
    };

    Some(res)
}
