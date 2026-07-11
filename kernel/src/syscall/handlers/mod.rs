pub mod process;
pub mod memory;
pub mod ipc;
pub mod vfs;
pub mod posix;

use shared::status::Status;

pub fn sys_write(fd: usize, ptr: usize, len: usize) -> usize {
    crate::log_debug!("SYSCALL_WRITE", "fd={}, ptr={:#x}, len={}", fd, ptr, len);
    if fd == 1 || fd == 2 {
        if ptr == 0 || len == 0 {
            return 0;
        }

        let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
        let l0_pa = if let Some(t) = thread_ptr {
            let proc_id = unsafe { (*t).process_id };
            if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
                proc.l0_user_pa
            } else {
                0
            }
        } else {
            0
        };

        for i in 0..len {
            let user_va = ptr + i;
            let pa = if l0_pa != 0 {
                match crate::arch::translate_user_va(l0_pa, user_va) {
                    Some(p) => p,
                    None => {
                        crate::log_error!("SYSCALL_WRITE", "Invalid user memory address: {:#x}", user_va);
                        return i;
                    }
                }
            } else {
                user_va
            };

            let kernel_va = crate::mm::mmu::pa_to_kernel_va(pa);
            let byte = unsafe { *(kernel_va as *const u8) };
            if byte == b'\n' {
                crate::arch::console_putchar(b'\r');
            }
            crate::arch::console_putchar(byte);
        }
        len
    } else {
        Status::NotAllowed.to_raw() as usize
    }
}
