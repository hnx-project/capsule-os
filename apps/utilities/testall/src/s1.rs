//! S1: POSIX identity / env / clock / locale surface checks.
//!
//! These exercise the new `libc` wrappers (`getuid`, `gettimeofday`,
//! `umask`, `setenv`, `getenv`, `unsetenv`, `sysconf`, `setlocale`).
//! They all run from the same userspace process so we don't need to
//! spawn a child just to verify wrappers.

use libc;

unsafe fn cstr_eq(a: *const u8, b: &str) -> bool {
    if a.is_null() {
        return b.is_empty();
    }
    let bs = b.as_bytes();
    for i in 0..bs.len() {
        if *a.add(i) != bs[i] {
            return false;
        }
    }
    *a.add(bs.len()) == 0
}

/// Read a NUL-terminated byte slice into a fixed-size buffer.
unsafe fn read_cstr_into(p: *const u8, buf: &mut [u8]) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut i = 0;
    while i < buf.len() {
        let b = *p.add(i);
        if b == 0 {
            return i;
        }
        buf[i] = b;
        i += 1;
    }
    i
}

fn env_lookup(name: &str) -> Option<[u8; 96]> {
    let mut name_c = [0u8; 32];
    let nb = name.as_bytes();
    if nb.len() > name_c.len() {
        return None;
    }
    name_c[..nb.len()].copy_from_slice(nb);
    let p = unsafe { libc::getenv(name_c.as_ptr()) };
    if p.is_null() {
        return None;
    }
    let mut buf = [0u8; 96];
    let n = unsafe { read_cstr_into(p, &mut buf) };
    let mut out = [0u8; 96];
    out[..n].copy_from_slice(&buf[..n]);
    Some(out)
}

fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if a[i] != b[i] {
            return false;
        }
    }
    true
}

fn bytes_starts_with(a: &[u8], prefix: &[u8]) -> bool {
    if a.len() < prefix.len() {
        return false;
    }
    for i in 0..prefix.len() {
        if a[i] != prefix[i] {
            return false;
        }
    }
    true
}

pub fn test_s1_run(t: &mut crate::TestRunner) {
    // 1. Identity — hard-coded to 0 in the single-tenant kernel.
    t.run("getuid_is_zero", unsafe { libc::getuid() == 0 });
    t.run("getgid_is_zero", unsafe { libc::getgid() == 0 });
    t.run("geteuid_is_zero", unsafe { libc::geteuid() == 0 });
    t.run("getegid_is_zero", unsafe { libc::getegid() == 0 });
    t.run("getpid_nonzero", unsafe { libc::getpid() > 0 });
    t.run("getppid_zero_or_pid", unsafe {
        let ppid = libc::getppid();
        ppid == 0 || ppid != libc::getpid()
    });
    t.run("setsid_returns_pid",
        unsafe { libc::setsid() == libc::getpid() });
    t.run("getsid_returns_pid",
        unsafe { libc::getsid(0) == libc::getpid() });
    t.run("getpgrp_returns_pid",
        unsafe { libc::getpgid(0) == libc::getpid() });
    t.run("setpgid_returns_zero",
        unsafe { libc::setpgid(0, 0) == 0 });

    // 2. umask round-trip
    unsafe {
        let old = libc::umask(0o600);
        // After the swap, the previous mask (which should be the
        // initial 0o022 mask) is returned; the second swap then
        // restores it.  Verify the second restore yields the
        // value we just stored (0o600).
        let now = libc::umask(0o022);
        t.run("umask_swap_matches", now == 0o600);
    }

    // 3. ttyname / std-fd -> "/dev/tty"
    t.run("ttyname_stdin",
        unsafe { cstr_eq(libc::ttyname(0), "/dev/tty") });
    t.run("ttyname_stdout",
        unsafe { cstr_eq(libc::ttyname(1), "/dev/tty") });

    // 4. sysconf numbers
    t.run("sysconf_pagesize",
        unsafe { libc::sysconf(30) == 4096 || libc::sysconf(47) == 4096 });
    t.run("sysconf_openmax",
        unsafe { libc::sysconf(4) == 64 });

    // 5. setlocale returns "C"
    t.run("setlocale_returns_c",
        unsafe { cstr_eq(libc::setlocale(0, core::ptr::null()), "C") });

    // 6. gettimeofday fills a positive seconds field.
    let mut tv = libc::PosixTimeval { tv_sec: -1, tv_usec: -1 };
    unsafe { libc::gettimeofday(&mut tv, core::ptr::null_mut()); }
    t.run("gettimeofday_fills",
        tv.tv_sec >= 0 && tv.tv_usec >= 0);

    // 7. env (PATH/HOME) seeded by env_init.
    let path = env_lookup("PATH");
    t.run("env_path_set",
        path.map(|v| bytes_starts_with(&v, b"/system/bin")).unwrap_or(false));
    let home = env_lookup("HOME");
    t.run("env_home_set",
        home.map(|v| bytes_eq(&v[..5], b"/root")).unwrap_or(false));
    let user = env_lookup("USER");
    t.run("env_user_set",
        user.map(|v| bytes_eq(&v[..4], b"root")).unwrap_or(false));

    // 8. setenv + getenv + unsetenv round-trip
    let rv = unsafe {
        libc::setenv(
            b"HNX_TEST_VAR\0".as_ptr(),
            b"hello123\0".as_ptr(),
        )
    };
    // Diagnostic: the return value isn't observable in testall
    // directly, but we can use it as an [PASS] / [FAIL] tweak.
    t.run("env_setenv_returns_zero", rv == 0);
    let roundtrip = env_lookup("HNX_TEST_VAR");
    t.run("env_setget_roundtrip",
        roundtrip.map(|v| bytes_eq(&v[..8], b"hello123")).unwrap_or(false));
    unsafe {
        libc::unsetenv(b"HNX_TEST_VAR\0".as_ptr());
    }
    t.run("env_unsetenv_roundtrip",
        env_lookup("HNX_TEST_VAR").is_none());
}
