//! `libtty` — EL0 line discipline for stdin/stdout/stderr.
//!
//! Mirrors Linux's `n_tty` line discipline (the `n_tty.c` rules: ERASE,
//! KILL, EOF, INTR, ECHO, ECHOE, ECHOK) but implements them in EL0
//! instead of EL1 because CapsuleOS is a microkernel: the kernel
//! exposes raw 1-byte reads on `fd = 0` and all line-editing policy
//! lives in this crate.
//!
//! API in three pieces:
//!
//! 1. [`TtyLine::read_line`] — cooked-mode read used by shells and
//!    interactive REPLs.  Returns one terminated line.
//!
//! 2. [`TtyLine::read_byte_raw`] — single-byte passthrough read for
//!    tools that want to drive their own line discipline (or read
//!    protocol-level control characters).
//!
//! 3. [`TtyLine::write`] — convenience wrapper for the
//!    write-to-stdout pattern.  All output goes through `fd = 1`
//!    so the kernel sees the same byte stream regardless of which
//!    process invoked the call.
//!
//! All syscalls go through the kernel-builtin UART path (`fd = 0`
//! for input, `fd = 1` for output) so this crate works for any
//! EL0 process that holds the canonical stdin/stdout handles.

#![no_std]

extern crate libc;
extern crate libcapsule;

use core::fmt;

/// Maximum line length supported by the EL0 line discipline.  Linux
/// uses `MAX_CANON = 255`; we follow the same value so that long
/// commands aren't silently truncated.
pub const LINE_MAX: usize = 255;

/// Standard ASCII control codes.  Centralised so the shell-side
/// `readline` and tty-side `libtty` agree on what they mean.
pub const CTRL_C: u8 = 0x03;
pub const CTRL_D: u8 = 0x04;
pub const CTRL_U: u8 = 0x15;
pub const CTRL_W: u8 = 0x17;
pub const DEL: u8 = 0x7f;
pub const BS: u8 = 0x08;
pub const ESC: u8 = 0x1b;
pub const CR: u8 = b'\r';
pub const LF: u8 = b'\n';

/// Errors surfaced by the line-discipline helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtyError {
    IoError,
    Closed,
}

impl fmt::Display for TtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TtyError::IoError => f.write_str("tty I/O error"),
            TtyError::Closed => f.write_str("tty closed"),
        }
    }
}

pub type TtyResult<T> = Result<T, TtyError>;

/// EL0 line-discipline state.  Combining the editing buffer, the
/// history ring, and the small helper that flushes bytes to the
/// console UART.
pub struct TtyLine {
    buf: [u8; LINE_MAX],
    len: usize,
    history: [[u8; LINE_MAX]; HISTORY_MAX],
    history_lens: [usize; HISTORY_MAX],
    history_count: usize,
    history_pos: usize,
}

const HISTORY_MAX: usize = 16;

impl TtyLine {
    pub const fn new() -> Self {
        Self {
            buf: [0u8; LINE_MAX],
            len: 0,
            history: [[0u8; LINE_MAX]; HISTORY_MAX],
            history_lens: [0usize; HISTORY_MAX],
            history_count: 0,
            history_pos: 0,
        }
    }

    /// Borrow the most recently edited line.  The buffer is valid
    /// until the next call to `read_line` / `history_*`.
    pub fn line(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Read one edited line from stdin in cooked mode.  Returns the
    /// line length on Enter / Ctrl-D, or 0 on Ctrl-C / empty Ctrl-D.
    ///
    /// Behaviour matches Linux `n_tty` cooked mode:
    ///   * `\n` / `\r` → terminate, echo `\r\n`.
    ///   * `0x7f` / `0x08` → erase previous char, echo `\b \b`.
    ///   * `0x15` (Ctrl-U) → kill entire line, echo `\r\n` + prompt.
    ///   * `0x17` (Ctrl-W) → erase previous word.
    ///   * `0x04` (Ctrl-D) → flush, return 0 if buffer empty.
    ///   * `0x03` (Ctrl-C) → echo `^C\r\n`, return 0.
    ///   * `0x1b` (ESC) → consume ANSI escape sequence (CSI only).
    ///   * `0x20..=0x7e` → append + echo.
    ///
    /// The shell is responsible for drawing the prompt *before* the
    /// call — `libtty` only echoes the characters the user types.
    pub fn read_line(&mut self) -> TtyResult<usize> {
        self.len = 0;
        self.history_pos = self.history_count;

        loop {
            let mut byte = [0u8; 1];
            let n = read(0, &mut byte)?;
            if n == 0 {
                return Ok(self.len);
            }
            let c = byte[0];

            match c {
                LF | CR => {
                    write_all(1, b"\r\n")?;
                    if self.len > 0 {
                        self.push_history();
                    }
                    return Ok(self.len);
                }
                CTRL_C => {
                    write_all(1, b"^C\r\n")?;
                    self.len = 0;
                    return Ok(0);
                }
                CTRL_D => {
                    if self.len == 0 {
                        return Ok(0);
                    }
                    // Mid-line Ctrl-D: flush the buffer so the line
                    // can be processed early.  Linux does the same.
                    let line = self.len;
                    self.push_history();
                    self.len = 0;
                    return Ok(line);
                }
                DEL | BS => {
                    if self.len > 0 {
                        self.len -= 1;
                        write_all(1, b"\x08 \x08")?;
                    }
                }
                CTRL_U => {
                    // Erase the entire line.  Linux convention:
                    // CR + LF + re-prompt prefix is the shell's job.
                    self.len = 0;
                    write_all(1, b"\r\n")?;
                    return Ok(0);
                }
                CTRL_W => {
                    self.erase_word();
                }
                ESC => {
                    // CSI sequences for arrow keys etc.  Read up to
                    // two more bytes; bail out on timeout.
                    let mut seq = [0u8; 2];
                    if read(0, &mut seq[..1]).is_ok() && seq[0] == b'[' {
                        let _ = read(0, &mut seq[1..2]);
                        match seq[1] {
                            b'A' => {
                                self.history_up();
                                self.redraw();
                            }
                            b'B' => {
                                self.history_down();
                                self.redraw();
                            }
                            _ => {}
                        }
                    }
                }
                0x20..=0x7e => {
                    if self.len < LINE_MAX {
                        self.buf[self.len] = c;
                        self.len += 1;
                        write_all_stdout(&[c])?;
                    }
                }
                _ => {
                    // Drop other control characters silently.
                }
            }
        }
    }

    /// Read a single byte without any line editing.  Pass-through
    /// for tools that want raw stdin semantics.
    pub fn read_byte_raw(&mut self) -> TtyResult<u8> {
        let mut byte = [0u8; 1];
        let n = read(0, &mut byte)?;
        if n == 0 {
            Err(TtyError::Closed)
        } else {
            Ok(byte[0])
        }
    }

    /// Convenience wrapper around `write(1, ...)`.
    pub fn write(&self, data: &[u8]) -> TtyResult<()> {
        write_all(1, data)
    }

    fn push_history(&mut self) {
        if self.history_count > 0 {
            let last = self.history_count - 1;
            if self.history_lens[last] == self.len
                && self.history[last][..self.len] == self.buf[..self.len]
            {
                return;
            }
        }
        let slot = if self.history_count < HISTORY_MAX {
            let s = self.history_count;
            self.history_count += 1;
            s
        } else {
            for i in 1..HISTORY_MAX {
                self.history[i - 1] = self.history[i];
                self.history_lens[i - 1] = self.history_lens[i];
            }
            HISTORY_MAX - 1
        };
        self.history[slot] = self.buf;
        self.history_lens[slot] = self.len;
    }

    fn history_up(&mut self) {
        if self.history_count == 0 || self.history_pos == 0 {
            return;
        }
        self.history_pos -= 1;
        self.load_history();
    }

    fn history_down(&mut self) {
        if self.history_pos >= self.history_count {
            return;
        }
        self.history_pos += 1;
        if self.history_pos >= self.history_count {
            self.buf = [0u8; LINE_MAX];
            self.len = 0;
        } else {
            self.load_history();
        }
    }

    fn load_history(&mut self) {
        let idx = self.history_pos;
        self.buf = self.history[idx];
        self.len = self.history_lens[idx];
    }

    /// Erase the previous word (Ctrl-W).  Mirrors bash's behaviour:
    /// scan backwards over whitespace, then over non-whitespace.
    fn erase_word(&mut self) {
        // Skip trailing whitespace.
        while self.len > 0 && (self.buf[self.len - 1] == b' ' || self.buf[self.len - 1] == b'\t') {
            self.len -= 1;
            let _ = write_all(1, b"\x08 \x08");
        }
        // Now walk back over the word characters.
        while self.len > 0 && self.buf[self.len - 1] != b' ' && self.buf[self.len - 1] != b'\t' {
            self.len -= 1;
            let _ = write_all(1, b"\x08 \x08");
        }
    }

    /// Repaint the prompt + buffer after a history navigation.  Uses
    /// CR + ANSI clear-to-EOL so the previous input (which the kernel
    /// has already echoed) is overwritten cleanly.
    fn redraw(&self) {
        let mut out = [0u8; LINE_MAX + 32];
        let prompt = b"osh$ ";
        let mut off = 0;
        out[off] = b'\r';
        off += 1;
        out[off..off + prompt.len()].copy_from_slice(prompt);
        off += prompt.len();
        out[off..off + self.len].copy_from_slice(&self.buf[..self.len]);
        off += self.len;
        // \x1b[K = ESC + '[' + 'K'  (3 bytes).
        out[off..off + 3].copy_from_slice(b"\x1b[K");
        off += 3;
        let _ = write_all_stdout(&out[..off]);
    }
}

// ---------------------------------------------------------------------------
// Thin syscall wrappers.  Defined here so the line discipline module
// doesn't depend on every EL0 process implementing libstd-style file
// IO.  Direct svc #0 traps.
// ---------------------------------------------------------------------------

fn read(fd: usize, buf: &mut [u8]) -> TtyResult<usize> {
    let ret = libcapsule::syscall!(
        shared::syscall_nums::SYSCALL_READ,
        fd,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(TtyError::IoError)
    } else {
        Ok(ret)
    }
}

fn write_all(fd: usize, data: &[u8]) -> TtyResult<()> {
    let mut written = 0;
    while written < data.len() {
        let ret = libcapsule::syscall!(
            shared::syscall_nums::SYSCALL_WRITE,
            fd,
            data[written..].as_ptr() as usize,
            data.len() - written,
            0,
            0,
            0
        );
        if (ret as isize) <= 0 {
            return Err(TtyError::IoError);
        }
        written += ret as usize;
    }
    Ok(())
}

/// stdout-only convenience used by the line-discipline helpers so
/// callers don't have to thread `fd = 1` through.
fn write_all_stdout(data: &[u8]) -> TtyResult<()> {
    write_all(1, data)
}