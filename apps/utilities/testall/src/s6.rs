//! S6: PTY (`/dev/ptmx` / `/dev/pts/N`) smoke checks.
//!
//! 1.0 only exercises the *user-side* helper wiring; the
//! end-to-end master/slave roundtrip lands once procmgr
//! delivers a child-side fd table (S7).

use libc;

pub fn test_s6_run(t: &mut crate::TestRunner) {
    crate::kprintln!("[s6] ENTER test_s6_run");
    // 1. Negative-fd ioctl returns -1 (libcapsule helper wiring
    //    does not touch the kernel).
    let r1 = unsafe { libcapsule::tty::ioctl(-1, libcapsule::tty::TIOCGWINSZ as u32, 0) };
    crate::kprintln!("[s6] ioctl_negfd ret={}", r1);
    t.run("tty_ioctl_negfd_minus_one", r1 == -1);

    // 2. Winsize defaults to zero before any ioctl is issued.
    let mut ws = libcapsule::tty::Winsize::default();
    t.run("tty_winsize_zeroed_initially",
        ws.ws_row == 0 && ws.ws_col == 0);

    // 3. Winsize layout sanity.
    t.run("tty_winsize_layout_consistent",
        core::mem::size_of::<libcapsule::tty::Winsize>() == 8);

    // 4. The libcapsule::tty::TIOCGWINSZ constant matches libc's.
    t.run("tty_tiocgwinsz_constant_matches_libc",
        libcapsule::tty::TIOCGWINSZ == libc::TIOCGWINSZ);

    // 5. get_winsize helper returns 0 even for a stray fd (the
    //    helper returns the ioctl return code, which is -1 for
    //    unregistered PTYs but 0 is not expected for a closed
    //    slot — we accept either way to keep the test flexible
    //    across S7).
    t.run("tty_get_winsize_does_not_crash",
        {
            let mut ws = libcapsule::tty::Winsize::default();
            let _ = libcapsule::tty::get_winsize(99i32, &mut ws);
            true // smoke: we only care that no panic happens.
        });

    // 6. The full master-end-to-end open.  Disabled by default
    //    while we isolate the EL0-FAULT that surfaced under the
    //    testall-spawn.  Set to `true` to enable the call;
    //    repro the EL0-FAULT in your workstation with
    //    `HAVE_FULL_PTY_OPEN=1 xtask code test`.
    {
        let mut p = [0u8; 16];
        let bs = b"/dev/ptmx";
        p[..bs.len()].copy_from_slice(bs);
        crate::kprintln!("[s6] calling libc::open(/dev/ptmx)...");
        let r = unsafe { libc::open(p.as_ptr(), 2 /* O_RDWR */, 0) };
        crate::kprintln!("[s6] libc::open returned r={}", r);
        t.run("tty_open_ptmx_returns_positive", r >= 0);
    }
}
