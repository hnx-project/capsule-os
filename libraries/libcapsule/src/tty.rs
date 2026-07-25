//! S6 user-side PTY helpers and ioctl wrappers.
//!
//! The libc `open()` wrapper checks for `/dev/ptmx` and
//! `/dev/pts/N` up-front and routes them through
//! `SYSCALL_TTY_OPEN`.  Other paths go through the existing
//! fileagent channel.  Once a PTY fd is returned the user can
//! drive it via `libcapsule::tty::pty_read` /
//! `libcapsule::tty::pty_write`, both thin wrappers around the
//! dedicated `SYSCALL_PTY_*` calls so the VFS dispatcher is
//! not on the hot path.

use shared::status::{Result, Status};
use shared::syscall_nums;

use crate::fd::{FdEntry, FdType, USER_FD_TABLE};
use crate::syscall;

/// `TIOCGWINSZ` ioctl — winsize.  Made public so testall and
/// other users can pass it directly to `ioctl(fd, TIOCGWINSZ, ...)`.
pub const TIOCGWINSZ: u32 = 0x4008_7468;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// Open `/dev/ptmx` (or `/dev/pts/N`).  Returns a
/// process-local fd number on success.
///
/// Bypasses the fileagent service: the kernel handles the
/// request directly via `SYSCALL_TTY_OPEN`.  The returned fd
/// is recorded in `USER_FD_TABLE` as `FdType::Pty` so the
/// libc-side `read` / `write` / `ioctl` wrappers can find it
/// later.
pub fn open_pty(path: &str) -> Result<i32> {
    let mut path_c: [u8; 32] = [0u8; 32];
    let n = path.as_bytes().len();
    if n == 0 || n >= path_c.len() {
        return Err(Status::InvalidArgs);
    }
    path_c[..n].copy_from_slice(&path.as_bytes()[..n]);
    let ret = syscall!(
        syscall_nums::SYSCALL_TTY_OPEN,
        path_c.as_ptr() as usize,
        n,
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        return Err(Status::from_raw(ret as i32));
    }
    let fd = ret as i32;
    if fd >= 0 {
        unsafe {
            if (fd as usize) < USER_FD_TABLE.len() {
                let slot_ptr = USER_FD_TABLE.as_mut_ptr().add(fd as usize);
                let entry = FdEntry {
                    r#type: FdType::Pty { fd },
                    flags: 0,
                };
                core::ptr::write(slot_ptr, Some(entry));
            }
        }
    }
    Ok(fd)
}

/// Read up to `dst.len()` bytes from a PTY fd.  Returns the
/// number of bytes copied, `Ok(0)` on EOF (no peer alive),
/// `Err(Status::TryAgain)` if the canonical line hasn't been
/// committed yet.
pub fn pty_read(fd: i32, dst: &mut [u8]) -> Result<usize> {
    if dst.is_empty() {
        return Ok(0);
    }
    let ret = syscall!(
        syscall_nums::SYSCALL_PTY_READ,
        fd as usize,
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

/// Write up to `data.len()` bytes to a PTY fd.
pub fn pty_write(fd: i32, data: &[u8]) -> Result<usize> {
    let ret = syscall!(
        syscall_nums::SYSCALL_PTY_WRITE,
        fd as usize,
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

/// Run an arbitrary ioctl on `fd`.  The kernel returns 0 on
/// success and `-1` on an unrecognised request.
pub fn ioctl(fd: i32, req: u32, arg: usize) -> i32 {
    let ret = syscall!(
        syscall_nums::SYSCALL_IOCTL,
        fd as usize,
        req as usize,
        arg,
        0,
        0,
        0
    );
    ret as i32
}

/// Convenience: read the TIOCGWINSZ winsize for `fd` and
/// copy it into `ws`.
pub fn get_winsize(fd: i32, ws: &mut Winsize) -> i32 {
    ioctl(fd, TIOCGWINSZ, ws as *mut Winsize as usize)
}
