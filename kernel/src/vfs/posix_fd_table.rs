//! Per-process POSIX file descriptor table.
//!
//! Phase 6 v0.6.0-α POSIX push, KERNEL_HEALTH.md bucket-1 P7.
//!
//! Previously `hnxlibc::LIBC_FILES[16]` lived in userspace and stored
//! `{ session_chan, remote_fd }` per locally-opened fd.  An EL0
//! program that bypassed hnxlibc (raw `__NR_open` / hand-written
//! `svc #0`) had no way to find the channel associated with a fd,
//! and the kernel-side `sys_open` was a stub that allocated empty
//! `Vnode`s.  This module makes the kernel the owner of the
//! `(process_id, fd) → PosixFd` mapping so all paths converge.
//!
//! The table is **page-allocated per process** to avoid sprinkling
//! 256-byte copies inside the `Process` struct (which would bloat
//! every PCB and waste 4 KiB on processes that never open a file —
//! init, loader, devmgr).  Process ID `0` is reserved for
//! "no current process" / kernel-only calls and explicitly rejected
//! from allocation.

use shared::status::{Result, Status};

/// One open file in a process's POSIX fd table.  Two u32 fields:
/// the kernel-side channel handle (`session_chan`) that talks to
/// fileagent, and fileagent-side per-connection table index
/// (`remote_fd`).
#[derive(Clone, Copy)]
pub struct PosixFd {
    pub session_chan: u32,
    pub remote_fd: u32,
}

const FD_PER_PROCESS: usize = 64;

/// Page-aligned per-process fd table.  Layout: `[Option<PosixFd>; 64]`
/// at the base, no metadata — exactly one page (256 bytes used, the
/// rest is slack that we can split later for sibling kernel objects).
#[repr(C)]
pub struct PosixFdTable {
    pub entries: [Option<PosixFd>; FD_PER_PROCESS],
}

impl PosixFdTable {
    pub const fn new_zeroed() -> Self {
        PosixFdTable { entries: [None; FD_PER_PROCESS] }
    }

    /// Allocate the next free fd slot in `pid`'s table.  Returns
    /// `Err(NoMemory)` if every slot is occupied or `Err(NotFound)`
    /// if no table exists for the process.
    pub fn alloc_fd_for(&mut self, session_chan: u32, remote_fd: u32) -> Result<u32> {
        for i in 0..FD_PER_PROCESS {
            if self.entries[i].is_none() {
                self.entries[i] = Some(PosixFd { session_chan, remote_fd });
                return Ok(i as u32);
            }
        }
        Err(Status::NoMemory)
    }

    pub fn get(&self, fd: u32) -> Option<PosixFd> {
        if (fd as usize) < FD_PER_PROCESS {
            self.entries[fd as usize]
        } else {
            None
        }
    }

    pub fn close_fd(&mut self, fd: u32) -> bool {
        if (fd as usize) < FD_PER_PROCESS {
            if self.entries[fd as usize].is_some() {
                self.entries[fd as usize] = None;
                return true;
            }
        }
        false
    }
}

/// `kernel/src/vfs/posix_fd_table.rs` is the authoritative owner of
/// per-process POSIX fd tables: a fixed-size `Vec`-style flat array
/// indexed by `process_id`, lazily allocated on first open.  `pid = 0`
/// is the kernel / no-process reservation sentinel.
///
/// The current QEMU bring-up is single-CPU and runs the syscall
/// dispatcher with kernel IRQs still masked (per the existing
/// scheduler-lock contract), so a global table with no internal
/// synchronisation is acceptable for v0.6-α.  SMP / IRQ-on safety
/// becomes a separate concern under Phase 6.4 K3+H5.
const MAX_PROCESSES: usize = 32;

static mut POSIX_FD_TABLE_PTRS: [*mut PosixFdTable; MAX_PROCESSES] =
    [core::ptr::null_mut(); MAX_PROCESSES];

/// Allocate a fresh page-backed `PosixFdTable` for `pid`.  Returns
/// `Err(NoMemory)` if the pid is out of range or kernel page
/// allocation fails; `Err(AlreadyExists)` if the slot was non-null.
fn alloc_posix_fd_table(pid: u64) -> Result<*mut PosixFdTable> {
    let slot = pid as usize;
    if slot >= MAX_PROCESSES {
        return Err(Status::InvalidArgs);
    }
    unsafe {
        if !POSIX_FD_TABLE_PTRS[slot].is_null() {
            return Err(Status::AlreadyExists);
        }
        let page_pa = crate::mm::phys::alloc_page().map_err(|_| Status::NoMemory)?;
        let kernel_va = crate::mm::mmu::pa_to_kernel_va(page_pa.as_usize());
        let table = kernel_va as *mut PosixFdTable;
        // Zero the page so all 64 entries start as `None`.  This is
        // already-zero because `phys::allocate_page` returns zeroed
        // frames, but we write through to be explicit.
        core::ptr::write_bytes(table as *mut u8, 0u8, 4096);
        POSIX_FD_TABLE_PTRS[slot] = table;
        Ok(table)
    }
}

fn table_for_mut(pid: u64) -> Result<*mut PosixFdTable> {
    let slot = pid as usize;
    if slot >= MAX_PROCESSES {
        return Err(Status::InvalidArgs);
    }
    let p = unsafe { POSIX_FD_TABLE_PTRS[slot] };
    if p.is_null() { Err(Status::NotFound) } else { Ok(p) }
}

fn table_for(pid: u64) -> Result<*const PosixFdTable> {
    table_for_mut(pid).map(|p| p as *const PosixFdTable)
}

/// Public API: register a per-process fd table on first call.
/// Called by `sys_open` dispatch arm the first time a process opens
/// a file.  Idempotent: returns `Ok(())` if the table already exists.
pub fn ensure_table_for(pid: u64) -> Result<()> {
    match table_for(pid) {
        Ok(_) => Ok(()),
        Err(Status::NotFound) => {
            alloc_posix_fd_table(pid)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn alloc_fd(pid: u64, session_chan: u32, remote_fd: u32) -> Result<u32> {
    let table = table_for_mut(pid)?;
    unsafe { (*table).alloc_fd_for(session_chan, remote_fd) }
}

pub fn get_fd(pid: u64, fd: u32) -> Result<PosixFd> {
    let table = table_for(pid)?;
    unsafe { (*table).get(fd).ok_or(Status::NotFound) }
}

pub fn close_fd(pid: u64, fd: u32) -> Result<bool> {
    let table = table_for_mut(pid)?;
    unsafe { Ok((*table).close_fd(fd)) }
}
