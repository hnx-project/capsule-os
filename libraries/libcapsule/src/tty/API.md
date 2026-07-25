# 📁 `libcapsule::tty` — User-side PTY helpers and ioctl wrappers

## Status
[Active (使用中)]

## Name
**`libcapsule::tty`** — thin userspace wrapper around the S6 PTY syscalls (`SYSCALL_TTY_OPEN` / `SYSCALL_PTY_READ` / `SYSCALL_PTY_WRITE` / `SYSCALL_IOCTL`).

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `libcapsule::syscall!` | Raw syscall primitive; every function in this module is a thin wrapper around one or two syscalls. |
| `libcapsule::fd::{FdEntry, FdType, USER_FD_TABLE}` | Records the freshly-opened PTY fd into `USER_FD_TABLE` so the libc-side `read` / `write` / `ioctl` wrappers can find it later. |
| `libraries::libc` | Calls into `libcapsule::tty::open_pty` from `libc::open` when the path starts with `/dev/ptmx` or `/dev/pts/N`; uses `pty_read` / `pty_write` for PTY-typed fds; uses `ioctl` for `TIOCGWINSZ` / friends. |
| `kernel::syscall::handlers::tty` | The kernel-side handlers this module targets. |
| `kernel::object::tty` | The in-kernel `PtySlot` storage. |

## Core Definition
This module exists so the libc layer can expose `/dev/ptmx` and `/dev/pts/N` opens without dragging kernel-side `PtyId` semantics up to userspace. All the heavy lifting (path classification, `PtySlot` allocation, fd_table installation) is handled by the kernel's `SYSCALL_TTY_OPEN` dispatcher; userspace only needs to forward the call and remember the returned fd in `USER_FD_TABLE`.

Once a PTY fd is installed, all subsequent `read` / `write` / `ioctl` traffic goes through the dedicated `SYSCALL_PTY_READ` / `SYSCALL_PTY_WRITE` / `SYSCALL_IOCTL` syscalls so the VFS dispatcher is not on the hot path.

## Exposed Interfaces

### Constants
| Symbol | Purpose |
|--------|---------|
| `pub const TIOCGWINSZ: u32 = 0x4008_7468` | ioctl number for window-size query. Re-exported so `testall` and `osh` can pass it directly to `ioctl()`. |

### Types
| Symbol | Purpose |
|--------|---------|
| `pub struct Winsize { ws_row: u16, ws_col: u16, ws_xpixel: u16, ws_ypixel: u16 }` | Wire format for `TIOCGWINSZ`. `#[repr(C)]` so it matches the C struct userspace programs expect. |

### Path → fd
| Function | Purpose |
|----------|---------|
| `pub fn open_pty(path: &str) -> Result<i32>` | Calls `SYSCALL_TTY_OPEN` with the path bytes; on success, records the fd into `USER_FD_TABLE` as `FdType::Pty { fd }`; returns the fd (positive) on success or `Err(Status::InvalidArgs / NotFound)`. |

### Read / Write
| Function | Purpose |
|----------|---------|
| `pub fn pty_read(fd: i32, dst: &mut [u8]) -> Result<usize>` | `SYSCALL_PTY_READ` wrapper; returns the byte count read. |
| `pub fn pty_write(fd: i32, data: &[u8]) -> Result<usize>` | `SYSCALL_PTY_WRITE` wrapper; returns the byte count written. |

### ioctl
| Function | Purpose |
|----------|---------|
| `pub fn ioctl(fd: i32, req: u32, arg: usize) -> i32` | `SYSCALL_IOCTL` wrapper. Returns 0 on success, `-1` on an unrecognised request. |
| `pub fn get_winsize(fd: i32, ws: &mut Winsize) -> i32` | Convenience: `ioctl(fd, TIOCGWINSZ, ws as *mut Winsize as usize)`. |

## Notes / Future Work

* `open_pty` currently accepts paths up to 32 bytes (`path_c: [u8; 32]`). Longer paths are rejected with `Status::InvalidArgs`. This matches `libc::open`'s short-circuit buffer size but should grow once a real `path` VMO is in place.
* `Winsize` does not yet have an OPOST `LineSettings` counterpart surfaced to userspace; `TCGETS` / `TCSETS` currently return 0 (no payload). S8 will hand back the actual struct.