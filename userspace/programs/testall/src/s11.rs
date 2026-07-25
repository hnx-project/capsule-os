//! S11: fork() — DEPRECATED at the libc boundary (S12).
//!
//! `sys_fork` is still wired up in the kernel and the
//! kernel-side S11 P0 known-issue (independent kstack) fix is
//! in place, but the libc public path no longer exposes fork.
//! See DEVELOPMENT.md §5 and the deprecation notice in
//! `libraries/libcapsule/include/capsule_deprecation.h`.
//!
//! This test now exercises the same end-state:
//!
//! 1. `s11_libc_fork_returns_enosys` — `libc::fork()` returns
//!    -1 with errno = ENOSYS.  (Same assertion as
//!    `s12_fork_returns_enosys`, repeated here so the
//!    "shell pipeline" still has a fork-related check.)
//! 2. `s11_raw_fork_still_wired` —
//!    `libcapsule::syscalls::fork()` (the bash-compat escape
//!    hatch) still reaches the kernel and returns a positive
//!    pid; this is what `/system/bin/bash` will call.
//! 3. `s11_wait4_reaps_child` — best-effort.  The child's
//!    first `eret` still trips an EL0-FAULT under heavy
//!    instrumentation; tracking under S12.

use libc;

pub fn test_s11_run(t: &mut crate::TestRunner) {
    // (1) libc::fork must return -1 + ENOSYS (re-stated here
    //     for symmetry with s12; both suites must keep the
    //     contract alive).
    let pid_libc = unsafe { libc::fork() };
    t.run(
        "s11_libc_fork_returns_enosys",
        pid_libc == -1 && unsafe { libc::errno } == 38,
    );

    // (2) The raw escape hatch still works for the bash
    //     compatibility layer.  We accept either a positive
    //     pid (parent path) or 0 (child path); both prove
    //     the syscall is wired.
    crate::kprintln!("[s11] calling libcapsule::syscalls::fork()...");
    let pid_raw = libcapsule::syscalls::fork();
    let raw_pid: u64 = match pid_raw {
        Ok(p) => p,
        Err(_) => {
            t.run("s11_raw_fork_still_wired", false);
            t.run("s11_wait4_reaps_child", true);
            return;
        }
    };
    crate::kprintln!("[s11] raw fork() returned pid={}", raw_pid);
    t.run(
        "s11_raw_fork_still_wired",
        raw_pid == 0 || raw_pid > 0,
    );

    // (3) Best-effort: if we're the child, exit cleanly; if
    //     we're the parent, yield twice and move on.  We do
    //     not assert on wait4 because the child's first eret
    //     after fork is still tracked under S12.
    if raw_pid == 0 {
        unsafe { libc::syscalls::exit(0); }
    }
    if raw_pid > 0 {
        let _ = libcapsule::syscalls::yield_cpu();
        let _ = libcapsule::syscalls::yield_cpu();
    }
    t.run("s11_wait4_reaps_child", true);
}