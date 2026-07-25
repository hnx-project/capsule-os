# 📁 `kernel::object::tty` — Pseudo-Terminal (PTY) Kernel Object

## Status
[Active (使用中)]

## Name
**Pseudo-Terminal (PTY) kernel object** — backs `open("/dev/ptmx")` for the bash / osh / procmgr S6 work.

## Dependencies & Related Components
| Component | Relationship |
|-----------|-------------|
| `kernel::object::handle_table` | `KernelObject::Tty(Box<PtySlot>)` is registered here via `tty::alloc_pty()` returning a `PtyId`. |
| `kernel::syscall::handlers::tty` | The `SYSCALL_TTY_OPEN` / `SYSCALL_PTY_READ` / `SYSCALL_PTY_WRITE` / `SYSCALL_IOCTL` paths call directly into this module's public API. |
| `kernel::syscall::handlers::process` | `sys_dup2` and `dispatch_pipe_io` walk the per-process `fd_table` and forward `FdEntry::Tty { .. }` to `tty::close_pty` / `tty::master_read` etc. |
| `libraries::libcapsule::tty` | Userspace wrapper that issues the raw syscalls listed above. |
| `libraries::libc` | Routes `open("/dev/ptmx")` and `ioctl(fd, TIOCGWINSZ, …)` to libcapsule, not fileagent. |

## Core Definition
The module owns a static table of up to `MAX_PTYS = 8` `PtySlot` records. Each slot is a paired master/slave pair:

* Writes from the **master** end land in a cook/line-edit buffer (`line_buf`) which the **slave** end reads out once a `\n` (or `EOF`) is seen — canonical-mode discipline.
* Writes from the **slave** end echo through `outbox` to the **master** if `LineSettings::echo` is set.
* Reads on either end block until the appropriate buffer has data — blocking is implemented via simple spin loops in 1.0; **an event channel replacement is tracked as a TODO and is the main outstanding concern of §7.**

The module is self-contained: `crate::object::handle_table::KernelObject` references a `PtySlot` by `PtyId(u16)`. No scheduler lock is taken inside `PtySlot` methods — callers own the per-thread scheduling decision.

## Exposed Interfaces

### Types
| Symbol | Purpose |
|--------|---------|
| `pub const MAX_PTYS: usize = 8` | Soft cap on simultaneous PTYs. |
| `pub const PTY_BUF_SIZE: usize = 4096` | Per-direction ring buffer length. |
| `pub struct PtyId(pub u16)` | Opaque PTY identifier — valid in the range `[0, MAX_PTYS)`. |
| `pub enum TtyRole { Master, Slave }` | Role tag for `FdEntry::Tty`. |
| `pub enum PathOp { AllocMaster, OpenSlave(u16), Controlling }` | Outcome of `path_lookups()`. |
| `pub struct LineSettings { icanon, echo, isig, opost, ixon }` | Cooked-mode / post-processing flags; copyable. |
| `pub struct Winsize { ws_row, ws_col, ws_xpixel, ws_ypixel }` | Wire-format for `TIOCGWINSZ`. |
| `pub struct PtySlot { … }` | The internal record (private fields). One per allocated PTY. |

### Lifecycle
| Function | Purpose |
|----------|---------|
| `pub fn alloc_pty() -> Result<PtyId>` | Allocate a fresh master-end PTY with `n_master = 1`, `master_opened = true`. Returns `Status::NoMemory` if the table is full. |
| `pub fn open_slave(id: PtyId) -> Result<()>` | Mark the slave end as opened and bump `n_slave`. |
| `pub fn close_pty(id: PtyId, is_master: bool)` | Decrement `n_master` or `n_slave`; releases the slot when both reach zero. |
| `pub fn find(id: PtyId) -> Option<&'static PtySlot>` | Read-only lookup for diagnostics / ioctl payload. |

### Data path
| Function | Purpose |
|----------|---------|
| `pub fn master_read(id, dst) -> Result<usize>` | Drain `outbox` into `dst`. |
| `pub fn master_write(id, src) -> usize` | Append into `line_buf` (canonical mode) or pass-through. |
| `pub fn slave_read(id, dst) -> Result<usize>` | Drain `line_buf` once a `\n` is committed. |
| `pub fn slave_consume(id, n)` | Acknowledge `n` bytes consumed by `slave_read`. |
| `pub fn slave_write(id, src) -> usize` | Echo bytes into `outbox` and possibly wake `master_waiting`. |

### Path dispatcher
| Function | Purpose |
|----------|---------|
| `pub fn path_lookups(path: &str) -> Option<PathOp>` | Resolve `/dev/ptmx` / `/dev/pts/N` / `/dev/tty` strings into a `PathOp`. Returns `None` for unrecognised paths. |

### State
| Symbol | Purpose |
|--------|---------|
| `pub static mut PTYS: [Option<PtySlot>; MAX_PTYS]` | Static PTY table. Accessed only from the syscall path; no concurrent writers in 1.0. |
| `pub static NEXT_PTY_ID: AtomicU32` | Monotonic counter for `alloc_pty`. |

## Invariants

1. `n_master ≥ 0`, `n_slave ≥ 0`. Both reaching 0 frees the slot back to `PTYS`.
2. `master_opened == true` ⇔ at least one `open("/dev/ptmx")` call has succeeded on this PTY.
3. `line_active` becomes true the moment a master write sees any byte and stays true until a `\n` is committed or the buffer overflows.
4. `line_buf` is FIFO with at most `PTY_BUF_SIZE` bytes pending before `master_write` returns the partial count.

## Known Limitations / Future Work

* **Blocking semantics** in 1.0 are implemented via short spin loops on the caller thread (acceptable because the syscall path is single-threaded). A proper event-channel replacement (`IdleFlags` + `wait_for_event`) is tracked under S10's idle-flag work.
* `PathOp::Controlling` (`/dev/tty`) currently behaves like `/dev/ptmx`; the real controlling-TTY lookup lands in S7/S11.
* `LineSettings` is fully populated but `isig` (INTR / QUIT) and `ixon` (XON/XOFF) are not yet wired to signal delivery.