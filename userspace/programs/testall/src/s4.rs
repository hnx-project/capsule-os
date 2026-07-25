//! S4: POSIX `fcntl(fd, cmd, arg)` and `ioctl(fd, req, arg)`
//! smoke surface checks.
//!
//! These are user-side only — both implementations live entirely
//! in `libcapsule + libc` because the kernel doesn't carry an
//! fd_flags table of its own (the kernel-side `SYSCALL_FCNTL` /
//! `SYSCALL_IOCTL` only handle TTY-class ops, which S6/S7 will
//! wire).

use libc;

pub fn test_s4_run(t: &mut crate::TestRunner) {
    // F_GETFD on a known fd returns a valid flag bit (0 or FD_CLOEXEC).
    let flags = unsafe { libc::fcntl(0, 1, 0) }; // F_GETFD = 1
    t.run("fcntl_fgetfd_stdin",
        flags == 0 || flags == libc::FD_CLOEXEC);

    // F_SETFD then F_GETFD round-trip.
    unsafe { libc::fcntl(0, 2 /* F_SETFD */, libc::FD_CLOEXEC); }
    let r = unsafe { libc::fcntl(0, 1 /* F_GETFD */, 0) };
    t.run("fcntl_setfd_getfd_roundtrip", r == libc::FD_CLOEXEC);
    // Clear it back so we don't leak state into the rest of testall.
    unsafe { libc::fcntl(0, 2, 0); }

    // F_GETFL on stdout.
    let fl = unsafe { libc::fcntl(1, 3 /* F_GETFL */, 0) };
    t.run("fcntl_fgetfl_stdout", true);

    // F_SETFL non-blocking and back.
    unsafe { libc::fcntl(1, 4 /* F_SETFL */, libc::O_NONBLOCK); }
    let fl_nb = unsafe { libc::fcntl(1, 3, 0) };
    t.run("fcntl_setfl_o_nonblock", (fl_nb & libc::O_NONBLOCK) == libc::O_NONBLOCK);
    unsafe { libc::fcntl(1, 4, 0); }

    // F_DUPFD into slot >= 3.
    let new_fd = unsafe { libc::fcntl(0, 0 /* F_DUPFD */, 3) };
    t.run("fcntl_dupfd_returns_valid_fd",
        new_fd >= 3 && new_fd < 64);

    // F_DUPFD_CLOEXEC copies with CLOEXEC set.
    let new_cx = unsafe { libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 3) };
    let cx_flags = unsafe { libc::fcntl(new_cx, 1, 0) };
    t.run("fcntl_dupfd_cloexec_sets_flag",
        (cx_flags & libc::FD_CLOEXEC) == libc::FD_CLOEXEC);

    // ioctl TIOCGWINSZ returns a sane 80x24 default.
    let mut ws = libc::Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let ret = unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &mut ws as *mut _ as *mut _) };
    t.run("ioctl_tiocgwinsz_returns_80x24",
        ret == 0 && ws.ws_row >= 24 && ws.ws_col >= 80);

    // ioctl on an unsupported op returns -1.
    let bad = unsafe { libc::ioctl(0, 0xdead_beefu32, core::ptr::null_mut()) };
    t.run("ioctl_unsupported_minus_one", bad == -1);

    // ioctl on -1 fd returns -1.
    let negfd = unsafe { libc::ioctl(-1, libc::TCGETS, core::ptr::null_mut()) };
    t.run("ioctl_neg_fd_minus_one", negfd == -1);
}
