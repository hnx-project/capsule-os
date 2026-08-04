/// `readline` — EL0 line editor for the osh shell.
///
/// In the commercial-OS model (Linux `n_tty.c`, FreeBSD `ttydisc`),
/// the line discipline is a library that sits between the kernel's
/// raw-byte stdin and the user's application.  CapsuleOS keeps that
/// split: kernel `sys_read(fd = 0, ...)` returns raw bytes, and
/// `libtty::TtyLine` provides the cooked-mode editor.  This crate
/// is a thin wrapper that gives the shell a `read_line` function
/// that prints "osh$ " before blocking.

extern crate libc;
extern crate libcapsule;
extern crate libtty;

pub struct Readline {
    tty: libtty::TtyLine,
}

impl Readline {
    pub fn new() -> Self {
        Self {
            tty: libtty::TtyLine::new(),
        }
    }

    /// Read one edited line from stdin.  Emits the `osh$ ` prompt
    /// before blocking, then delegates the line discipline to
    /// `libtty::TtyLine::read_line`.  Returns the number of valid
    /// bytes in the captured line (0 on Ctrl-C / empty Ctrl-D).
    pub fn read_line(&mut self) -> Result<usize, ()> {
        // Render the prompt via libcapsule's existing `libstd::io`
        // path so we don't depend on std's per-process fds for the
        // echo channel.  fd = 1 is the kernel-builtin UART stdout.
        libstd_io_print("osh$ ");
        match self.tty.read_line() {
            Ok(n) => Ok(n),
            Err(_) => Err(()),
        }
    }

    /// Borrow the most recently edited line.
    pub fn line(&self) -> &[u8] {
        self.tty.line()
    }
}

fn libstd_io_print(s: &str) {
    let _ = libcapsule::syscall!(
        libcapsule::SYSCALL_WRITE,
        1,
        s.as_ptr() as usize,
        s.len(),
        0,
        0,
        0
    );
}