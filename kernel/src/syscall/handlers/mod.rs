pub mod process;
pub mod memory;
pub mod ipc;
pub mod vfs;
pub mod device;
pub mod mmio;
pub mod tty;

use shared::status::Status;

pub fn sys_write(fd: usize, ptr: usize, len: usize) -> usize {
    if fd == 1 || fd == 2 {
        if ptr == 0 || len == 0 {
            return 0;
        }

        let thread_ptr = unsafe { crate::task::scheduler::SCHEDULER.get_current_thread_ptr() };
        let l0_pa = if let Some(t) = thread_ptr {
            let proc_id = unsafe { (*t).process_id };
            if let Some(proc) = crate::task::process::find_process_mut(proc_id) {
                proc.page_table.l0_pa()
            } else {
                0
            }
        } else {
            0
        };

        // If we don't have a translation table for the calling
        // process (rare — e.g. mid-boot or the prelude before a
        // process is wired up), refuse the write rather than
        // deref the user VA directly.  Direct deref would crash
        // the kernel in EL1 with EC=0x24 and FAR=user_va, which
        // looks like an EL0 fault in the test runner.
        if l0_pa == 0 {
            crate::log_error!(
                "SYSCALL_WRITE",
                "fd={}: no l0_pa for caller; cannot safely translate user buffer",
                fd
            );
            return 0;
        }

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

            let byte_opt = unsafe {
                let kv = crate::arch::mmu_facade::pa_to_kernel_va(pa);
                if kv == 0 {
                    None
                } else {
                    Some(*(kv as *const u8))
                }
            };
            let byte = match byte_opt {
                Some(b) => b,
                None => {
                    crate::log_error!(
                        "SYSCALL_WRITE",
                        "no kernel mapping for user_va={:#x} pa={:#x}",
                        user_va,
                        pa
                    );
                    return i;
                }
            };
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
