/// S8: minimal readline — line editing with history.
///
/// The kernel already provides cooked-mode echo and backspace
/// for fd 0 (Console UART).  This module layers on top:
///
///   - History ring buffer (up/down arrows)
///   - Ctrl+C / Ctrl+D handling
///   - Buffers the full line for the pipeline runner
///
/// Left/right cursor movement is deferred until CapsuleOS gains
/// proper termios / PTY support (S6.5+).  For now the kernel's
/// backspace-or-append model is sufficient.
use crate::env::Environment;

const HISTORY_MAX: usize = 16;
const BUF_SIZE: usize = 256;

pub struct Readline {
    buf: [u8; BUF_SIZE],
    len: usize,
    history: [[u8; BUF_SIZE]; HISTORY_MAX],
    history_lens: [usize; HISTORY_MAX],
    history_count: usize,
    history_pos: usize,
}

impl Readline {
    pub fn new() -> Self {
        Self {
            buf: [0u8; BUF_SIZE],
            len: 0,
            history: [[0u8; BUF_SIZE]; HISTORY_MAX],
            history_lens: [0usize; HISTORY_MAX],
            history_count: 0,
            history_pos: 0,
        }
    }

    /// Read one edited line from stdin.  Returns `Ok(n)` where `n`
    /// is the number of valid bytes in the internal buffer.  `n == 0`
    /// means EOF or Ctrl+C.
    pub fn read_line<E: Environment>(&mut self, env: &E) -> Result<usize, ()> {
        self.len = 0;
        self.history_pos = self.history_count;
        self.buf = [0u8; BUF_SIZE];
        env.write_stdout(b"osh$ ");

        loop {
            let mut byte = [0u8; 1];
            match env.read(0, &mut byte) {
                Ok(1) => {}
                _ => continue,
            }
            let c = byte[0];

            match c {
                b'\r' | b'\n' => {
                    env.write_stdout(b"\r\n");
                    if self.len > 0 {
                        self.push_history();
                    }
                    return Ok(self.len);
                }
                0x03 => {
                    env.write_stdout(b"^C\r\n");
                    self.len = 0;
                    return Ok(0);
                }
                0x04 => {
                    if self.len == 0 {
                        return Ok(0);
                    }
                }
                b'\x7f' | 0x08 => {
                    if self.len > 0 {
                        self.len -= 1;
                    }
                }
                b'\x1b' => {
                    let mut seq = [0u8; 2];
                    if env.read(0, &mut seq).is_ok() && seq[0] == b'[' {
                        match seq[1] {
                            b'A' => self.history_up(env),
                            b'B' => self.history_down(env),
                            _ => {}
                        }
                    }
                }
                0x20..=0x7e => {
                    if self.len < BUF_SIZE - 1 {
                        self.buf[self.len] = c;
                        self.len += 1;
                    }
                }
                _ => {}
            }
        }
    }

    pub fn line(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Redraw the prompt line from scratch.  Used after history
    /// navigation to replace the kernel-echoed garbage.
    fn redraw<E: Environment>(&self, env: &E) {
        let mut out = [0u8; BUF_SIZE + 16];
        let mut off = 0;
        out[off] = b'\r';
        off += 1;
        out[off..off + 5].copy_from_slice(b"osh$ ");
        off += 5;
        out[off..off + self.len].copy_from_slice(&self.buf[..self.len]);
        off += self.len;
        out[off..off + 4].copy_from_slice(b"\x1b[K");
        off += 4;
        env.write_stdout(&out[..off]);
    }

    fn history_up<E: Environment>(&mut self, env: &E) {
        if self.history_count == 0 || self.history_pos == 0 {
            return;
        }
        self.history_pos -= 1;
        self.load_history();
        self.redraw(env);
    }

    fn history_down<E: Environment>(&mut self, env: &E) {
        if self.history_pos >= self.history_count {
            return;
        }
        self.history_pos += 1;
        if self.history_pos >= self.history_count {
            self.buf = [0u8; BUF_SIZE];
            self.len = 0;
        } else {
            self.load_history();
        }
        self.redraw(env);
    }

    fn load_history(&mut self) {
        let idx = self.history_pos;
        self.buf = self.history[idx];
        self.len = self.history_lens[idx];
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
}
