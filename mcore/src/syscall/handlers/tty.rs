//! S6 syscall handlers for the kernel PTY object.
//!
//! Three entry points:
//!   * `sys_tty_open(path_ptr, path_len, flags)` resolves
//!     `/dev/ptmx` to a fresh PtyId and returns an fd whose
//!     in-kernel counterpart is `process::fd_table[idx] =
//!     FdEntry::Tty { role: Master }`.
//!   * `sys_tty_read` / `sys_tty_write` drive the FdEntry::Tty
//!     through its master / slave role tag.
//!   * `sys_tty_ioctl(fd, req, arg)` walks the per-process
//!     `fd_table`, locates the `FdEntry::Tty`, and dispatches
//!     the TTY-class ioctl (`TIOCGWINSZ` / `TCGETS` / ...).
//!
//! `tty_close` decrements the underlying PTY's refcount.
//!
//! ## Memory access discipline
//!
//! Per DEVELOPMENT.md §3 (`Strict Security Auditing`), all
//! user-supplied buffers and strings are routed through
//! `safe_copy_from_user` / `safe_copy_to_user` so the kernel
//! never dereferences a raw EL0 pointer.  The fixed 4 KiB
//! kernel scratch buffer caps the read/write span and lets
//! `safe_copy_*_user` translate the VA through the caller's
//! L0 page table before any byte touches PTY ring storage.

use shared::status::{Result, Status};
use crate::object::tty::{self, PtyId, PathOp};
use crate::syscall::handlers::ipc::{safe_copy_from_user, safe_copy_to_user};
use crate::task::process::{self, FdEntry, TtyRole};

/// Maximum bytes a single syscall will pull from / push to user
/// memory in one shot.  Matches the pipe ring buffer cap; we
/// reuse the same 4 KiB scratch on the kernel stack.
const TTY_XFER_LIMIT: usize = 4096;

/// Allocate a fresh PTY for `/dev/ptmx` and return the
/// per-process fd that references its master end.
pub fn sys_tty_open(path_ptr: usize, path_len: usize, _flags: u32) -> Result<u32> {
    if path_ptr == 0 || path_len == 0 || path_len > TTY_XFER_LIMIT {
        return Err(Status::InvalidArgs);
    }
    let caller_pid = process::current_process_id()
        .map_err(|_| Status::NotAllowed)?;
    let proc = process::find_process_mut(caller_pid)
        .ok_or(Status::NotAllowed)?;
    let l0_pa = proc.page_table.l0_pa();
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }

    let mut path_buf = [0u8; TTY_XFER_LIMIT];
    safe_copy_from_user(l0_pa, path_ptr, path_len, &mut path_buf[..path_len])?;
    let path_str = core::str::from_utf8(&path_buf[..path_len])
        .map_err(|_| Status::InvalidArgs)?;

    let op = match tty::path_lookups(path_str) {
        Some(o) => o,
        None => return Err(Status::NotFound),
    };

    match op {
        PathOp::AllocMaster => {
            let id = tty::alloc_pty()?;
            install_fd(id, TtyRole::Master)
        }
        PathOp::OpenSlave(n) => {
            let id = PtyId(n);
            tty::open_slave(id)?;
            install_fd(id, TtyRole::Slave)
        }
        PathOp::Controlling => {
            // S6 placeholder — S7 will replace this with the
            // real controlling-TTY lookup.  We treat it like
            // a fresh `ptmx` open for now.
            let id = tty::alloc_pty()?;
            install_fd(id, TtyRole::Master)
        }
    }
}

/// Helper: find the calling process, install a fresh fd_table
/// slot pointing at `id:role`, and return the fd number.
///
/// Slots `0..=2` are reserved for the kernel-builtin UART path
/// (stdin/stdout/stderr) — searching the table from index 3
/// keeps PTY fds from clashing with the console fds (B7/S7
/// procmgr std-fd handoff will move these reserved slots out of
/// `install_fd`'s reach).  See `process::USER_FD_BASE`.
fn install_fd(id: PtyId, role: TtyRole) -> Result<u32> {
    use crate::task::process::USER_FD_BASE;
    let caller_pid = process::current_process_id()
        .map_err(|_| Status::NotAllowed)?;
    let proc = process::find_process_mut(caller_pid)
        .ok_or(Status::NotAllowed)?;
    let slot_idx = proc.fd_table.iter().enumerate()
        .skip(USER_FD_BASE as usize)
        .find(|(_, s)| s.is_none())
        .map(|(i, _)| i)
        .ok_or(Status::NoMemory)?;
    proc.fd_table[slot_idx] = Some(FdEntry::Tty { pty: id, role });
    Ok(slot_idx as u32)
}

/// Read on a master or slave fd.  Master reads drain the
/// slave's output box; slave reads drain the cooked-mode
/// line buffer (delivered to userspace only when a `\n` has
/// completed the canonical line).
pub fn sys_tty_read(fd: u32, buf_ptr: usize, buf_len: usize) -> Result<usize> {
    if buf_ptr == 0 || buf_len == 0 {
        return Err(Status::InvalidArgs);
    }
    let caller_pid = process::current_process_id()
        .map_err(|_| Status::NotAllowed)?;
    let proc = process::find_process_mut(caller_pid)
        .ok_or(Status::NotAllowed)?;
    let l0_pa = proc.page_table.l0_pa();
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let (id, role) = match proc.fd_table.get(fd as usize) {
        Some(Some(FdEntry::Tty { pty, role })) => (*pty, *role),
        Some(Some(_)) => return Err(Status::InvalidArgs),
        _ => return Err(Status::NotFound),
    };

    let want = core::cmp::min(buf_len, TTY_XFER_LIMIT);
    let mut kernel_buf = [0u8; TTY_XFER_LIMIT];

    let n = match role {
        TtyRole::Master => tty::master_read(id, &mut kernel_buf[..want])?,
        TtyRole::Slave => {
            let n = tty::slave_read(id, &mut kernel_buf[..want])?;
            if n > 0 {
                tty::slave_consume(id, n);
            }
            n
        }
    };
    if n > 0 {
        safe_copy_to_user(l0_pa, &kernel_buf[..n], buf_ptr, n)?;
    }
    Ok(n)
}

/// Write on a master or slave fd.
pub fn sys_tty_write(fd: u32, buf_ptr: usize, buf_len: usize) -> Result<usize> {
    if buf_ptr == 0 || buf_len == 0 {
        return Err(Status::InvalidArgs);
    }
    let caller_pid = process::current_process_id()
        .map_err(|_| Status::NotAllowed)?;
    let proc = process::find_process_mut(caller_pid)
        .ok_or(Status::NotAllowed)?;
    let l0_pa = proc.page_table.l0_pa();
    if l0_pa == 0 {
        return Err(Status::InvalidArgs);
    }
    let (id, role) = match proc.fd_table.get(fd as usize) {
        Some(Some(FdEntry::Tty { pty, role })) => (*pty, *role),
        Some(Some(_)) => return Err(Status::InvalidArgs),
        _ => return Err(Status::NotFound),
    };

    let want = core::cmp::min(buf_len, TTY_XFER_LIMIT);
    let mut kernel_buf = [0u8; TTY_XFER_LIMIT];
    safe_copy_from_user(l0_pa, buf_ptr, want, &mut kernel_buf[..want])?;

    let n = match role {
        TtyRole::Master => tty::master_write(id, &kernel_buf[..want]),
        TtyRole::Slave => tty::slave_write(id, &kernel_buf[..want]),
    };
    Ok(n)
}

/// Close a master or slave fd.  Decrements the matching
/// refcount on the underlying PTY.
pub fn sys_tty_close(fd: u32) -> Result<()> {
    let caller_pid = process::current_process_id()
        .map_err(|_| Status::NotAllowed)?;
    let proc = process::find_process_mut(caller_pid)
        .ok_or(Status::NotAllowed)?;
    let slot = proc.fd_table.get_mut(fd as usize).ok_or(Status::NotFound)?;
    let entry = match slot.take() {
        Some(e) => e,
        None => return Err(Status::NotFound),
    };
    if let FdEntry::Tty { pty, role } = entry {
        tty::close_pty(pty, role == TtyRole::Master);
    }
    Ok(())
}

/// TTY-specific ioctl.  Currently wires:
///   * `TIOCGWINSZ` — copy the PTY's `Winsize` into the
///                   caller-supplied user VA.
///   * `TCGETS`      — return 0 (placeholder; S8 will hand
///                     back the actual `LineSettings`).
///   * `TCSETS`      — accept and discard (placeholder).
///   * other TTY ops return -1 to keep the syscall table
///     stable while we add more in subsequent stages.
pub fn sys_tty_ioctl(fd: u32, req: u32, arg_ptr: usize) -> Result<i32> {
    let caller_pid = process::current_process_id()
        .map_err(|_| Status::NotAllowed)?;
    let proc = process::find_process_mut(caller_pid)
        .ok_or(Status::NotAllowed)?;
    let l0_pa = proc.page_table.l0_pa();
    let (id, _role) = match proc.fd_table.get(fd as usize) {
        Some(Some(FdEntry::Tty { pty, role })) => (*pty, *role),
        Some(Some(_)) => return Err(Status::InvalidArgs),
        _ => return Err(Status::NotFound),
    };

    const TIOCGWINSZ: u32 = 0x4008_7468;
    const TCGETS: u32 = 0x5401;
    const TCSETS: u32 = 0x5402;
    const TCSETSW: u32 = 0x5403;
    const TCSETSF: u32 = 0x5404;
    const TIOCGPGRP: u32 = 0x5410;
    const TIOCSPGRP: u32 = 0x5411;
    const TIOCSCTTY: u32 = 0x2000_5310;
    const TIOCNOTTY: u32 = 0x2000_5311;

    match req {
        TIOCGWINSZ => {
            if arg_ptr == 0 || l0_pa == 0 {
                return Err(Status::InvalidArgs);
            }
            let p = match tty::find(id) {
                Some(p) => p,
                None => return Err(Status::NotFound),
            };
            let ws = p.winsize;
            let ws_bytes = unsafe {
                core::slice::from_raw_parts(
                    (&ws as *const tty::Winsize) as *const u8,
                    core::mem::size_of::<tty::Winsize>(),
                )
            };
            safe_copy_to_user(
                l0_pa,
                ws_bytes,
                arg_ptr,
                core::mem::size_of::<tty::Winsize>(),
            )?;
            Ok(0)
        }
        TCGETS | TCSETS | TCSETSW | TCSETSF | TIOCGPGRP | TIOCSPGRP
        | TIOCSCTTY | TIOCNOTTY => Ok(0),
        _ => Ok(-1),
    }
}

/// S6: variant that looks up the PTY by the *handle* stored
/// in the user's `USER_FD_TABLE` rather than by `process::fd_table`
/// index.  Reserved for S7 when we want to expose fd numbers
/// >63; for now the `fd_table`-based dispatcher is the only
/// path userspace takes.
#[allow(dead_code)]
pub fn ioctl_by_handle(_handle: u32, _req: u32, _arg_ptr: usize) -> Result<i32> {
    Err(Status::NotAllowed)
}