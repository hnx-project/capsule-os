//! # POSIX identity & clock surface (S1)
//!
//! Thin safe wrappers around the S1 syscall family.  These live in
//! `libcapsule` (not `libc`) because the latter is `#![no_std]`
//! C-ABI; `libcapsule` is already the user-side runtime and is
//! where every status-aware callers wants them.

use shared::status::Status;
use crate::syscalls;

#[inline]
fn negative_to_status(r: usize) -> Result<usize, Status> {
    if (r as isize) < 0 {
        Err(Status::from_raw(r as i32))
    } else {
        Ok(r)
    }
}

/// Returns the calling process's real user id.
pub fn getuid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETUID, 0, 0, 0, 0, 0, 0
    )
}
/// Effective user id.
pub fn geteuid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETEUID, 0, 0, 0, 0, 0, 0
    )
}
/// Real group id.
pub fn getgid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETGID, 0, 0, 0, 0, 0, 0
    )
}
/// Effective group id.
pub fn getegid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETEGID, 0, 0, 0, 0, 0, 0
    )
}
/// Parent process id.
pub fn getppid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETPPID, 0, 0, 0, 0, 0, 0
    )
}
/// Process group of the calling process.
pub fn getpgrp() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETPGRP, 0, 0, 0, 0, 0, 0
    )
}
/// Session id of the calling process.
pub fn getsid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GETSID, 0, 0, 0, 0, 0, 0
    )
}
/// Move the calling process into a brand-new session.  In our flat
/// model this returns the caller's own pid; bash needs the call to
/// succeed without ENOSYS.
pub fn setsid() -> Result<usize, Status> {
    let r = crate::syscall!(
        shared::syscall_nums::SYSCALL_SETSID, 0, 0, 0, 0, 0, 0
    );
    negative_to_status(r)
}
/// Join (or found, if `pgrp <= 0`) a process group.  Stub for
/// `setpgid(0, 0)`-shaped calls; returns the new pgrp (= own pid)
/// unconditionally.
pub fn setpgid(pid: usize, pgrp: usize) -> Result<usize, Status> {
    let r = crate::syscall!(
        shared::syscall_nums::SYSCALL_SETPGID, pid, pgrp, 0, 0, 0, 0
    );
    negative_to_status(r)
}

/// `struct timeval { tv_sec: i64, tv_usec: i64 }` — laid out so
/// the kernel can fill it directly via two `write_volatile` calls
/// without any temporary buffer.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

/// POSIX `gettimeofday(&mut tv, _)`.  Returns 0 on success;
/// `EINVAL` if `tv` is not a valid user VA (handled by the kernel).
pub fn gettimeofday(tv: &mut Timeval) -> Result<(), Status> {
    let r = crate::syscall!(
        shared::syscall_nums::SYSCALL_GETTIMEOFDAY,
        tv as *mut Timeval as usize,
        0, 0, 0, 0, 0
    );
    negative_to_status(r).map(|_| ())
}

/// Get the kernel thread id (POSIX `gettid()`).
pub fn gettid() -> usize {
    crate::syscall!(
        shared::syscall_nums::SYSCALL_GET_TID, 0, 0, 0, 0, 0, 0
    )
}
