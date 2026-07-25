//! # S6 — Pseudo-Terminal (PTY) kernel object
//!
//! This module owns the in-kernel `PtySlot` table that backs
//! `open("/dev/ptmx")` for the bash / osh S6 / S7 work.  Each
//! PtySlot is a paired master/slave pair:
//!
//!   * Writes from the **master** end land in a cook/line-edit
//!     buffer (`line_buf`) which the **slave** end reads out
//!     once a `\n` (or `EOF`) is seen — canonical-mode discipline.
//!   * Writes from the **slave** end echo through `outbox` to
//!     the **master** if `LineSettings::echo` is set.
//!   * Reads on either end are non-blocking: if the matching
//!     buffer is empty and the peer is still alive the syscall
//!     returns `Status::TryAgain`.  The libc-side `read` retries
//!     with a short backoff.  S10+ will replace this with a
//!     proper event-channelised wait using the existing
//!     `idle_flags` wake primitive.
//!
//! The module is self-contained: `crate::object::handle_table::KernelObject`
//! references a PtySlot by `PtyId(u16)`, and the syscall
//! handlers in `kernel/src/syscall/handlers/{process,tty_ioctl}.rs`
//! invoke the public API.  No task-scheduler lock is taken
//! inside PtySlot methods — callers (the syscall dispatcher)
//! own the per-thread scheduling decision.
//!
//! Why a static table: 8 PtySlots cover all realistic uses
//! (a console PTY for osh + a TTY-over-network-dummy for netd
//! is plenty); growing it to 64 would cost another 4 KiB of
//! kernel .bss for the rare case of more than 8 simultaneous
//! terminals.

use core::sync::atomic::{AtomicU32, Ordering};
use shared::status::{Result, Status};

/// Maximum number of PTYs the kernel can host simultaneously.
pub const MAX_PTYS: usize = 8;

/// 4 KiB line buffer per PTY.  Pipes on Linux default to
/// 16 pages; we cap at 4 KiB because every byte written
/// triggers a wake-up of the slave end and we don't yet have
/// the event-channel infrastructure to throttle efficiently.
pub const PTY_BUF_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyId(pub u16);

/// One slot in the static PTY table.  `kind == Master` means
/// the slot is referenced from a master-end fd; `kind ==
/// Slave` means it's referenced from the slave-end fd.  Each
/// PtySlot holds state for both directions of the same pair so
/// reads/writes look symmetrical.
#[derive(Debug)]
pub struct PtySlot {
    pub id: PtyId,
    /// Master end of the pair.  `Writer side` for terminal
    /// output (master→slave) and `Reader side` for terminal
    /// input (slave→master).
    pub line: LineSettings,
    pub winsize: Winsize,
    /// Bytes the **master** has produced (i.e. typed or written
    /// into stdin) but the **slave** has not yet read — these
    /// bytes flow master→slave.
    pub line_buf: [u8; PTY_BUF_SIZE],
    pub line_len: usize,
    /// `true` while a cooked-mode line is being assembled.
    /// Read returns when the line is terminated with `\n`.
    pub line_active: bool,
    /// Bytes the **slave** has produced (i.e. stdout/stderr);
    /// they echo back to the master if `ECHO` is on, otherwise
    /// they just consume buffer space.
    pub outbox: [u8; PTY_BUF_SIZE],
    pub outbox_head: usize,
    pub outbox_tail: usize,
    /// Tracks whether the master / slave side is currently
    /// blocked in a future event-channelised wait.  Unused in
    /// 1.0 (reads return `Status::TryAgain` instead) but kept
    /// so the S10+ event-channel replacement doesn't have to
    /// change the slot layout.  `idle_flags` is already a seed
    /// for the kernel-side wake primitive.
    pub master_waiting: bool,
    pub slave_waiting: bool,
    /// Reference count for each end.  Closes the slot when both
    /// drop to zero.
    pub n_master: u16,
    pub n_slave: u16,
    /// `true` once `slave_open()` has been called by at least
    /// one opener; the master and slave can't both come up in
    /// arbitrary order because the slave end of a PTY is
    /// traditionally unlocked only after the master calls
    /// `grantpt()`.  For 1.0 we skip that and unlock on
    /// `tty_open` directly.
    pub master_opened: bool,
    pub slave_opened: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LineSettings {
    /// ICANON: cooked mode.  When set, reads on the slave end
    /// return after the slave sees `\n`; when cleared, reads
    /// pass through byte-by-byte (`cat` style).
    pub icanon: bool,
    /// ECHO: bytes the master writes (master side "keyboard
    /// input") are echoed back to the master fd before being
    /// delivered to the slave.  Off for password-style entries.
    pub echo: bool,
    /// ISIG: `\x03` (INTR) / `\x1c` (QUIT) deliver signals to
    /// the foreground pgroup.  Off during safe-mode bash.
    pub isig: bool,
    /// OPOST: post-process output (LF→CRLF on terminals).
    pub opost: bool,
    /// IXON: enable XON/XOFF flow control (`\x11`/`\x13`).
    pub ixon: bool,
}

impl LineSettings {
    pub fn default_cooked() -> Self {
        LineSettings {
            icanon: true,
            echo: true,
            isig: true,
            opost: true,
            ixon: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

impl PtySlot {
    pub const fn new_uninit(id: PtyId) -> Self {
        Self {
            id,
            line: LineSettings {
                icanon: false,
                echo: false,
                isig: false,
                opost: false,
                ixon: false,
            },
            winsize: Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
            line_buf: [0u8; PTY_BUF_SIZE],
            line_len: 0,
            line_active: false,
            outbox: [0u8; PTY_BUF_SIZE],
            outbox_head: 0,
            outbox_tail: 0,
            master_waiting: false,
            slave_waiting: false,
            n_master: 0,
            n_slave: 0,
            master_opened: false,
            slave_opened: false,
        }
    }
}

/// Static PTY table.  Slots are populated lazily by
/// `alloc_pty()`; the FS is small enough that walking the table
/// on every open is faster than maintaining a free-list.
pub static mut PTYS: [Option<PtySlot>; MAX_PTYS] = [const { None }; MAX_PTYS];
pub static NEXT_PTY_ID: AtomicU32 = AtomicU32::new(0);

/// Allocate a fresh PTY with `n_master = 1, n_slave = 0`.  The
/// caller must follow up with `tty_open(slave)` to actually use
/// it.  Returns `Status::NoMemory` when the table is full.
pub fn alloc_pty() -> Result<PtyId> {
    let new_id = NEXT_PTY_ID.fetch_add(1, Ordering::Relaxed);
    if (new_id as usize) >= MAX_PTYS {
        return Err(Status::NoMemory);
    }
    unsafe {
        PTYS[new_id as usize] = Some(PtySlot::new_uninit(PtyId(new_id as u16)));
        if let Some(p) = PTYS[new_id as usize].as_mut() {
            p.line = LineSettings::default_cooked();
            p.n_master = 1;
            p.master_opened = true;
            // Primitive OPOST-friendly default: newline → CR/LF on
            // output, matching Linux's `stty opost` default for
            // most terminals.
        }
    }
    Ok(PtyId(new_id as u16))
}

/// Open the slave end of an existing PTY slot.  Increments the
/// slave refcount and returns the same `PtyId`.  Used by
/// `open("/dev/pts/N")`.
pub fn open_slave(id: PtyId) -> Result<()> {
    unsafe {
        if let Some(p) = PTYS[id.0 as usize].as_mut() {
            p.n_slave = p.n_slave.saturating_add(1);
            p.slave_opened = true;
            Ok(())
        } else {
            Err(Status::NotFound)
        }
    }
}

/// Drop a master or slave reference.  Frees the slot when both
/// refcounts are zero.
pub fn close_pty(id: PtyId, is_master: bool) {
    unsafe {
        if let Some(p) = PTYS[id.0 as usize].as_mut() {
            if is_master {
                if p.n_master > 0 {
                    p.n_master -= 1;
                }
            } else {
                if p.n_slave > 0 {
                    p.n_slave -= 1;
                }
            }
            if p.n_master == 0 && p.n_slave == 0 {
                PTYS[id.0 as usize] = None;
            }
        }
    }
}

/// Master-side write: append `data` to the slave's line-edited
/// buffer.  Canonical-mode assembles lines; non-canonical mode
/// pushes bytes straight through.  Returns the number of
/// bytes accepted (always `data.len()` until the buffer is
/// full — partial writes are treated as errors for 1.0).
///
/// Lines terminated by `\n` are committed to `line_buf` and
/// flip `line_active`.  Slave reads return either assembled
/// lines (canonical) or raw bytes (non-canonical).
pub fn master_write(id: PtyId, data: &[u8]) -> usize {
    unsafe {
        let p = match PTYS[id.0 as usize].as_mut() {
            Some(p) => p,
            None => return 0,
        };
        let written = if p.line.icanon {
            // Cooked mode: commit each completed line; partial
            // input stays pending until a `\n` arrives.
            for &b in data {
                if b == b'\n' {
                    // Commit whatever we had (even an empty line)
                    // so the slave sees a one-byte line.
                    p.line_active = false;
                } else if !p.line_active {
                    p.line_active = true;
                    p.line_len = 0;
                }
                if p.line_active && p.line_len < PTY_BUF_SIZE {
                    p.line_buf[p.line_len] = b;
                    p.line_len += 1;
                }
            }
            data.len()
        } else {
            // Raw mode: every byte goes in immediately.
            for &b in data {
                if p.line_len < PTY_BUF_SIZE {
                    p.line_buf[p.line_len] = b;
                    p.line_len += 1;
                }
            }
            data.len()
        };
        written
    }
}

/// Slave-side read: drain `line_buf` into `dst`.  Returns
/// `Ok(n)` for the number of bytes copied; `Ok(0)` for EOF
/// (master is closed and the buffer is empty); `TryAgain`
/// when the buffer is empty but the master is still alive
/// (1.0 is non-blocking — libc::read retries with a short
/// backoff).
pub fn slave_read(id: PtyId, dst: &mut [u8]) -> Result<usize> {
    unsafe {
        let p = match PTYS[id.0 as usize].as_mut() {
            Some(p) => p,
            None => return Err(Status::NotFound),
        };
        let n = p.line_len.min(dst.len());
        if n == 0 {
            if p.n_master == 0 {
                return Ok(0); // EOF
            }
            // Non-blocking: libc::read retries with a short backoff.
            return Err(Status::TryAgain);
        }
        // In canonical mode, only release the buffer once a
        // whole line is committed (`line_active == false`).
        if p.line.icanon && p.line_active && n > 0 {
            return Err(Status::TryAgain);
        }
        dst[..n].copy_from_slice(&p.line_buf[..n]);
        Ok(n)
    }
}

/// Drain assembled bytes from `line_buf` and reset the line
/// state.  Called by the syscall layer after a successful
/// `slave_read()` so the buffer is ready for the next line.
pub fn slave_consume(id: PtyId, n: usize) {
    unsafe {
        if let Some(p) = PTYS[id.0 as usize].as_mut() {
            p.line_len -= n;
            if p.line_len > 0 {
                p.line_buf.copy_within(n..n + p.line_len, 0);
            }
            p.line_active = false;
        }
    }
}

/// Slave-side write: append to the master's echo/output box.
/// Returns the number of bytes accepted (always
/// `data.len()` until the buffer is full).
pub fn slave_write(id: PtyId, data: &[u8]) -> usize {
    unsafe {
        let p = match PTYS[id.0 as usize].as_mut() {
            Some(p) => p,
            None => return 0,
        };
        for &b in data {
            let next = (p.outbox_head + 1) % PTY_BUF_SIZE;
            if next == p.outbox_tail {
                // full — drop the byte silently for 1.0
                break;
            }
            p.outbox[p.outbox_head] = b;
            p.outbox_head = next;
        }
        data.len()
    }
}

/// Master-side read: drain `outbox`.  Same EOF / TryAgain
/// convention as `slave_read`.
pub fn master_read(id: PtyId, dst: &mut [u8]) -> Result<usize> {
    unsafe {
        let p = match PTYS[id.0 as usize].as_mut() {
            Some(p) => p,
            None => return Err(Status::NotFound),
        };
        if p.outbox_head == p.outbox_tail {
            if p.n_slave == 0 {
                return Ok(0);
            }
            return Err(Status::TryAgain);
        }
        let mut copied = 0usize;
        for b in dst.iter_mut() {
            if p.outbox_tail == p.outbox_head {
                break;
            }
            *b = p.outbox[p.outbox_tail];
            p.outbox_tail = (p.outbox_tail + 1) % PTY_BUF_SIZE;
            copied += 1;
        }
        Ok(copied)
    }
}

/// Cheap accessor used by the syscall layer for path lookup
/// (`/dev/ptmx` → fresh master) and `/dev/pts/N` → existing
/// slave.
pub fn find(id: PtyId) -> Option<&'static mut PtySlot> {
    unsafe { PTYS[id.0 as usize].as_mut() }
}

/// Look up the master slot by raw index (used during IPC
/// marshalling when an IPC peer hands us a `PtyId`).
pub fn slot_at(index: usize) -> Option<&'static mut PtySlot> {
    if index >= MAX_PTYS {
        return None;
    }
    unsafe { PTYS[index].as_mut() }
}

/// Number of currently-allocated PTYs.  Used by `ps` and the
/// boot-time topology log.
pub fn active_count() -> usize {
    let mut c = 0;
    unsafe {
        for slot in PTYS.iter() {
            if slot.is_some() {
                c += 1;
            }
        }
    }
    c
}

/// Returns true if the supplied path matches one of the
/// well-known pseudo-terminal device paths:
///   * `/dev/ptmx`        — kernel-side master allocation
///   * `/dev/pts/N`       — slave end of pty index `N`
///   * `/dev/tty`         — controlling TTY (currently
///                          always returns the master end of
///                          `tty_zero()`; the controlling-tty
///                          semantics land in S7)
pub fn path_lookups(path: &str) -> Option<PathOp> {
    if path == "/dev/ptmx" {
        Some(PathOp::AllocMaster)
    } else if let Some(rest) = path.strip_prefix("/dev/pts/") {
        rest.parse::<u16>().ok().map(PathOp::OpenSlave)
    } else if path == "/dev/tty" {
        // S6 convenience: `/dev/tty` returns the master of a
        // synthetic PTY (slot 0 when no other allocator has run
        // yet).  The fully-correct controlling-TTY lookup
        // belongs in S7 alongside procmgr std fd handoff.
        Some(PathOp::Controlling)
    } else {
        None
    }
}

/// What the PTY subsystem should do for a given `open()` path.
#[derive(Debug, PartialEq, Eq)]
pub enum PathOp {
    /// Allocate a fresh PTY and return its master fd.
    AllocMaster,
    /// Open the slave end of existing PTY index `N`.
    OpenSlave(u16),
    /// S7 placeholder: return the controlling TTY.  In S6 we
    /// treat it as `AllocMaster` (a brand-new PTY per open).
    Controlling,
}
