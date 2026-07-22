//! # 📁 Process-Local File Descriptor (FD) Table Manager
//!
//! `fd` manages the process-local file descriptor mappings to underlying
//! native microkernel capability channels and connection handles.

use shared::status::{Status, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FdType {
    Console,
    File {
        channel_handle: usize,
        remote_fd: u32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct FdEntry {
    pub r#type: FdType,
    pub flags: i32,
}

#[no_mangle]
pub static mut USER_FD_TABLE: [Option<FdEntry>; 64] = [
    Some(FdEntry {
        r#type: FdType::Console,
        flags: 0,
    }),
    Some(FdEntry {
        r#type: FdType::Console,
        flags: 1,
    }),
    Some(FdEntry {
        r#type: FdType::Console,
        flags: 2,
    }),
    None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None, None, None,
    None,
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
