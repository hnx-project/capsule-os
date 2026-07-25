//! # 📁 Process-Local File Descriptor (FD) Table Manager
//!
//! `fd` manages the process-local file descriptor mappings to underlying
//! native microkernel capability channels and connection handles.

use shared::status::{Status, Result};
use shared::syscall_nums;

use crate::syscall;

/// POSIX `FD_CLOEXEC` bit — set by `fcntl(F_SETFD)` so the kernel
/// closes this fd when the process `execve()`s.
pub const FD_CLOEXEC: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FdType {
    Console,
    File {
        channel_handle: usize,
        remote_fd: u32,
    },
    /// S2: pipe-end fd.  The role discriminant uses the same
    /// values as `kernel::vfs::pipe::PipeRole` (0 = Read,
    /// 1 = Write).  Reads from a write-end or writes to a
    /// read-end fail at the libc::read / libc::write
    /// dispatcher level, so the S2 path only needs to
    /// distinguish role at the point of dispatch.
    Pipe {
        pipe: u32,
        role: i32,
    },
    /// TTY fd allocated through `SYSCALL_TTY_OPEN`.  The
    /// `fd` field is the same index as in the userspace
    /// `USER_FD_TABLE`; the kernel carries its own copy in
    /// the caller's process `fd_table` field.
    Pty {
        fd: i32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct FdEntry {
    pub r#type: FdType,
    /// Flag bits: bit 0 = `FD_CLOEXEC`.  Set on the user side so
    /// the kernel's `sys_execve` can scan the fd_table and
    /// silently close any CLOEXEC'd entries before the new
    /// program starts.
    pub flags: i32,
}

#[no_mangle]
pub static mut USER_FD_TABLE: [Option<FdEntry>; 64] = [
    Some(FdEntry { r#type: FdType::Console, flags: 0 }),
    Some(FdEntry { r#type: FdType::Console, flags: 0 }),
    Some(FdEntry { r#type: FdType::Console, flags: 0 }),
    None, None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None,
];

pub struct FdManager;

impl FdManager {
    /// Allocate a new process-local FD slot for a given FdEntry.
    pub fn allocate(entry: FdEntry) -> Result<i32> {
        unsafe {
            for i in 3..USER_FD_TABLE.len() {
                if USER_FD_TABLE[i].is_none() {
                    USER_FD_TABLE[i] = Some(entry);
                    return Ok(i as i32);
                }
            }
            Err(Status::NoMemory)
        }
    }

    /// Retrieve the FdEntry of a specific process-local FD.
    pub fn get(fd: i32) -> Result<FdEntry> {
        if fd < 0 || fd >= 64 {
            return Err(Status::InvalidArgs);
        }
        unsafe {
            USER_FD_TABLE[fd as usize].ok_or(Status::NotFound)
        }
    }

    /// Release and remove a process-local FD slot.
    pub fn release(fd: i32) -> Result<FdEntry> {
        if fd < 0 || fd >= 64 {
            return Err(Status::InvalidArgs);
        }
        unsafe {
            USER_FD_TABLE[fd as usize].take().ok_or(Status::NotFound)
        }
    }

    /// Duplicate a process-local FD slot into another specific slot.
    pub fn dup2(oldfd: i32, newfd: i32) -> Result<i32> {
        if oldfd < 0 || oldfd >= 64 || newfd < 0 || newfd >= 64 {
            return Err(Status::InvalidArgs);
        }
        unsafe {
            let entry = USER_FD_TABLE[oldfd as usize].ok_or(Status::NotFound)?;
            USER_FD_TABLE[newfd as usize] = Some(entry);
            Ok(newfd)
        }
    }
}

/// S2: read up to `dst.len()` bytes from the kernel-side pipe
/// identified by `id`.  `id` is the raw u16 handle returned by
/// `SYSCALL_PIPE`.  Returns the number of bytes copied,
/// `Ok(0)` on EOF (no writers alive), or
/// `Status::TryAgain` if the buffer is empty but writers
/// still exist (1.0 can't block, so libc::read should retry
/// with a short backoff).
pub fn pipe_read(id: u32, dst: &mut [u8]) -> Result<usize> {
    let ret = syscall!(
        syscall_nums::SYSCALL_PIPE_RW,
        (id as usize) & 0xFFFF,
        dst.as_mut_ptr() as usize,
        dst.len(),
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as usize)
    }
}

/// S2: write up to `data.len()` bytes to the kernel-side pipe
/// identified by `id`.  Returns the number of bytes accepted.
pub fn pipe_write(id: u32, data: &[u8]) -> Result<usize> {
    let ret = syscall!(
        syscall_nums::SYSCALL_PIPE_RW,
        (id as usize) & 0xFFFF | 0x10000,
        data.as_ptr() as usize,
        data.len(),
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as usize)
    }
}
