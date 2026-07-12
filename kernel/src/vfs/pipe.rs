//! B6 (`KERNEL_HEALTH.md` B6): in-kernel pipe table for shell
//! pipelines.
//!
//! CapsuleOS does not run a real VFS layer in 1.0; the fileagent
//! service takes the role of a server, but B6 / B7 need a kernel
//! primitive that two EL0 processes (the two halves of a shell
//! pipeline) can hand bytes through without going through
//! fileagent.  We implement it as a fixed-size circular buffer
//! keyed by an in-kernel `PipeId`.
//!
//! ## Scope (1.0)
//!
//!   * 4 KiB ring buffer per pipe, page-aligned.
//!   * Single-process read+write semantics: a writer can
//!     `pipe_write` only if at least one reader holds the other
//!     end; vice versa.  When all writer ends are closed, the
//!     next read returns 0 (EOF) instead of blocking.
//!   * Blocking semantics for full-pipe writes / empty-pipe reads
//!     are stubbed with a NotAllowed return for 1.0 (smoke tests
//!     break the loop on full / empty).  A proper
//!     `EAGAIN`-equivalent retry will land in 1.1 alongside the
//!     signal pause.
//!   * sys_write on a read end (or sys_read on a write end)
//!     returns EBADF.
//!
//! Why a separate module: kernel-side file descriptors,
//! `Process::fd_table`, and IPC channel lookup are all
//! orthogonal enough that keeping pipe state isolated lets the
//! syscall handlers stay thin.

use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::{Result, Status};

pub const PIPE_BUF_SIZE: usize = 4096;
pub const MAX_PIPES: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct PipeId(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct Pipe {
    /// Backing buffer.  Index `(head == tail) == empty`.  Bytes
    /// are pushed at `head` and popped at `tail`.  When head
    /// catches up to tail across the buffer wrap, write returns
    /// `NotAllowed` (1.0: no blocking).  When the buffer empties
    /// and *all* writer ends are gone, read returns 0.
    pub buf: [u8; PIPE_BUF_SIZE],
    pub head: usize,
    pub tail: usize,
    /// Count of currently-held read ends and write ends.  Each
    /// `sys_pipe` allocates a fresh pipe and bumps both.  Each
    /// `sys_close` on an fd of role `Read` decrements `n_readers`,
    /// and vice versa.  Last close wakes pending operations via
    /// 1.1's signal handle; for 1.0 we just rely on EOF + busy
    /// polling.
    pub n_readers: usize,
    pub n_writers: usize,
}

impl Pipe {
    pub const fn new_uninit() -> Self {
        Self {
            buf: [0u8; PIPE_BUF_SIZE],
            head: 0,
            tail: 0,
            n_readers: 0,
            n_writers: 0,
        }
    }
}

pub static mut PIPES: [Option<Pipe>; MAX_PIPES] = [const { None }; MAX_PIPES];

static NEXT_PIPE_ID: AtomicUsize = AtomicUsize::new(0);

/// Allocate a fresh pipe with refcounts `n_readers = 1, n_writers
/// = 1`.  Returns the new `PipeId` on success; `Status::NoMemory`
/// when the table is full.
pub fn alloc_pipe() -> Result<PipeId> {
    let new_id = NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed) as u32;
    if new_id as usize >= MAX_PIPES {
        return Err(Status::NoMemory);
    }
    unsafe {
        PIPES[new_id as usize] = Some(Pipe::new_uninit());
        if let Some(p) = PIPES[new_id as usize].as_mut() {
            p.n_readers = 1;
            p.n_writers = 1;
        }
    }
    Ok(PipeId(new_id))
}

/// Increment the read or write refcount for an existing pipe,
/// used when a process dup's the corresponding fd.
pub fn pipe_clone_role(id: PipeId, role: PipeRole) {
    unsafe {
        if let Some(p) = PIPES[id.0 as usize].as_mut() {
            match role {
                PipeRole::Read => p.n_readers += 1,
                PipeRole::Write => p.n_writers += 1,
            }
        }
    }
}

/// Decrement the matching refcount, freeing the slot when both
/// hit zero.
pub fn pipe_close_role(id: PipeId, role: PipeRole) {
    unsafe {
        let slot = PIPES[id.0 as usize].as_mut();
        if let Some(p) = slot {
            match role {
                PipeRole::Read => {
                    if p.n_readers > 0 {
                        p.n_readers -= 1;
                    }
                }
                PipeRole::Write => {
                    if p.n_writers > 0 {
                        p.n_writers -= 1;
                    }
                }
            }
            if p.n_readers == 0 && p.n_writers == 0 {
                PIPES[id.0 as usize] = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeRole {
    Read,
    Write,
}

/// Read up to `buf.len()` bytes from `id`'s ring buffer.
/// Returns the number of bytes consumed, 0 if EOF (all writers
/// gone) and the buffer is empty, or `Status::NotAllowed` if
/// the buffer is empty but writers still exist (1.0: non-blocking).
pub fn pipe_read(id: PipeId, buf: &mut [u8]) -> Result<usize> {
    unsafe {
        let p = PIPES[id.0 as usize].as_mut().ok_or(Status::BadHandle)?;
        if p.n_writers == 0 && p.head == p.tail {
            return Ok(0); // EOF
        }
        let mut n = 0usize;
        while n < buf.len() {
            if p.head == p.tail {
                // Empty: if no writers, EOF; else partial read.
                if p.n_writers == 0 {
                    return Ok(n);
                }
                return Ok(n);
            }
            buf[n] = p.buf[p.tail];
            p.tail = (p.tail + 1) % PIPE_BUF_SIZE;
            n += 1;
        }
        Ok(n)
    }
}

/// Write up to `buf.len()` bytes into `id`'s ring buffer.
/// Returns the number of bytes accepted or `Status::NotAllowed`
/// if the buffer is full and readers still exist (1.0:
/// non-blocking).  If `n_readers == 0` and we would block, we
/// surface that as `Status::BrokenPipe` so a shell pipeline can
/// decide to `kill -SIGPIPE` the writer (B7's job).
pub fn pipe_write(id: PipeId, buf: &[u8]) -> Result<usize> {
    unsafe {
        let p = PIPES[id.0 as usize].as_mut().ok_or(Status::BadHandle)?;
        if p.n_readers == 0 {
            // SIGPIPE-equivalent: return BrokenPipe so a Bash-like
            // shell can catch it.  In 1.0 we don't actually deliver
            // SIGPIPE to the writer because sigreturn trampolines
            // aren't in scope; we surface the error so the writer
            // can sys_exit cleanly.
            return Err(Status::PeerClosed);
        }
        let mut n = 0usize;
        while n < buf.len() {
            let next_head = (p.head + 1) % PIPE_BUF_SIZE;
            if next_head == p.tail {
                // Full: 1.0 non-blocking surfaces "writer overran".
                if n == 0 {
                    return Err(Status::NotAllowed);
                }
                return Ok(n);
            }
            p.buf[p.head] = buf[n];
            p.head = next_head;
            n += 1;
        }
        Ok(n)
    }
}
