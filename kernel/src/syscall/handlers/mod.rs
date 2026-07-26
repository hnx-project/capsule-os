pub mod process;
pub mod memory;
pub mod ipc;
pub mod vfs;
pub mod device;
pub mod mmio;
pub mod tty;

use shared::status::Status;

/// SYSCALL_WRITE: write `len` bytes from `ptr` (user VA) to the
/// kernel's stdout/stderr UART path (fd 1 and fd 2 only).
///
/// Implementation note: this dispatcher **copies the user buffer
/// out in one safe_copy_from_user call** rather than walking the
/// user page table byte-by-byte.  The 1.0 implementation used to
/// loop over `len` with one MMU walk + cache invalidation per
/// byte, which left a 1 KiB write at ~3–4 KiB syscall overhead
/// per call.  B6 (BULK_WRITE) replaces that with a single
/// page-walk loop driven by `safe_copy_from_user`'s already
/// page-aware copy.  A 1 KiB write now costs the same page
/// walks as a 1-byte write (one walk per page touched).
///
/// Per-call size cap: 4 KiB.  Larger writes are truncated; the
/// caller (libc::write) loops with consecutive sys_writes until
/// the buffer drains, so the visible behaviour matches POSIX.
/// Keeping the kernel-side staging buffer stack-allocated
/// avoids a heap allocation in the syscall hot path.
const WRITE_STAGE_BUF: usize = 4096;

pub fn sys_write(fd: usize, ptr: usize, len: usize) -> usize {
    if fd == 1 || fd == 2 {
        if ptr == 0 || len == 0 {
            return 0;
        }

        // Locate the caller's process and L0 translation base.
        // We can't call `get_current_thread_ptr` because that
        // itself takes the scheduler lock; the dispatcher holds
        // it here.
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

        let copy_len = core::cmp::min(len, WRITE_STAGE_BUF);
        let mut stage = [0u8; WRITE_STAGE_BUF];
        if let Err(_) = ipc::safe_copy_from_user(l0_pa, ptr, copy_len, &mut stage[..copy_len]) {
            crate::log_error!(
                "SYSCALL_WRITE",
                "safe_copy_from_user failed: fd={} ptr={:#x} len={}",
                fd,
                ptr,
                copy_len
            );
            return 0;
        }

        for &byte in &stage[..copy_len] {
            if byte == b'\n' {
                crate::arch::console_putchar(b'\r');
            }
            crate::arch::console_putchar(byte);
        }
        copy_len
    } else {
        Status::NotAllowed.to_raw() as usize
    }
}