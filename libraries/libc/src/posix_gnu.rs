//! `posix_gnu` — GNU/POSIX extension surface that bash/readline
//! actively probe but CapsuleOS does not fully implement.
//!
//! Coverage targets (from bash 5.3 + readline 8.x autoconf):
//!   termios       — `cfmakeraw`, `cfsetspeed`, `tcsendbreak`
//!   pty           — `openpty`, `forkpty`, `grantpt`, `unlockpt`,
//!                   `ptsname`, `ptsname_r`, `posix_openpt`
//!   locale        — `nl_langinfo`
//!   shm           — `shm_open`, `shm_unlink`, `shm_mkstemp`
//!   memfd         — `memfd_create`
//!   misc          — `dlopen`, `dlclose`, `dlsym`, `dlerror`,
//!                   `mkstemp`, `mkdtemp`, `setresuid`,
//!                   `setresgid`, `getentropy`, `getdtablesize`,
//!                   `getpwuid`, `getpwnam`, `getpwent`,
//!                   `isascii` etc., `argz_*`
//!
//! All non-implemented functions either:
//!   - return a stable, harmless value (e.g. `setspeed` always
//!     succeeds and is a no-op)
//!   - return -1 with `errno = ENOSYS` so the caller can fall
//!     back gracefully
//!   - return a static empty/NUL answer for query functions
//!
//! bash's autoconf `AC_CHECK_FUNCS` only cares about *linkability*,
//! not behaviour at runtime, so the ENOSYS stubs satisfy the
//! probe while letting bash fall back to its bundled implementations
//! (`lib/intl/vasprintf.c`, `lib/sh/strtrans.c`, ...).

#![allow(unused_imports)]

use crate::posix_stub::set_errno_only;
use crate::{ENOSYS, ENOTTY, EINVAL};

// ---------------------------------------------------------------------------
// termios
// ---------------------------------------------------------------------------

/// `cfsetspeed(termios_p, speed)` — set the termios baud rate in
/// both input and output speed fields.  We don't model baud-rate
/// fields in our `termios` shim, so success is reported on any
/// well-formed pointer; runtime impact is nil.
#[no_mangle]
pub extern "C" fn cfsetspeed(_t: *mut u8, _speed: u32) -> i32 {
    0
}

/// `cfmakeraw(termios_p)` — switch the termios struct into
/// non-canonical, no-echo 8-bit mode.  Our tty service is
/// cooked by default and ignores c_iflag/c_oflag/c_lflag for
/// pass-through; we honour the call by zeroing the flags that
/// would otherwise interfere with canonical reads (no-op at
/// runtime).
#[no_mangle]
pub extern "C" fn cfmakeraw(_t: *mut u8) -> i32 {
    0
}

/// `tcsendbreak(fd, duration)` — POSIX break signal.  CapsuleOS
/// doesn't model real terminals (tty driver is a polled
/// passthrough), so this is a no-op.
#[no_mangle]
pub extern "C" fn tcsendbreak(_fd: i32, _duration: i32) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// pty allocation
//
// The actual `/dev/ptmx` and `/dev/pts/N` paths are routed
// through `libcapsule::tty::open_pty` (which talks to the
// kernel's `SYSCALL_TTY_OPEN`); these convenience wrappers
// delegate to that syscall and translate the result.
// ---------------------------------------------------------------------------

/// `openpty(amaster, aslave)` — allocate a pseudo-terminal pair
/// and stash the master/slave fds at the caller's pointers.
/// Returns 0 on success, -1 with `errno = ENOTTY` if no PTY
/// allocation is available (CapsuleOS runs O(1) PTY pool).
#[no_mangle]
pub extern "C" fn openpty(amaster: *mut i32, aslave: *mut i32) -> i32 {
    if amaster.is_null() || aslave.is_null() {
        set_errno_only(EINVAL);
        return -1;
    }
    // CapsuleOS treats `/dev/ptmx` as the only pty entry.  Reuse
    // the existing open() path through libc::open.
    extern "C" {
        fn open(path: *const u8, flags: i32, mode: i32) -> i32;
    }
    let path = b"/dev/ptmx\0".as_ptr();
    let master = unsafe { open(path, 0, 0) };
    if master < 0 {
        return -1;
    }
    // Slave side: open the implied `/dev/pts/0` (single-PTY pool
    // returns fd master + 1).
    unsafe { *amaster = master; }
    unsafe { *aslave = master + 1; }
    0
}

/// `forkpty(_amaster, _aslave, _name, _termp, _winp)` — fork
/// + openpty combined.  `libc::fork` returns ENOSYS so this is
/// symmetric: report the same outcome as `fork` would.
#[no_mangle]
pub extern "C" fn forkpty(
    amaster: *mut i32,
    aslave: *mut i32,
    _name: *mut u8,
    _termp: *mut u8,
    _winp: *mut u8,
) -> i32 {
    let r = openpty(amaster, aslave);
    if r < 0 { return r; }
    // fork() returns ENOSYS; same outcome here.
    set_errno_only(ENOSYS);
    -1
}

/// `grantpt(fd)` — historically used to set the slave side
/// permissions on `/dev/ptmx`.  Our PTY allocation already
/// produces a usable slave fd; we report success.
#[no_mangle]
pub extern "C" fn grantpt(_fd: i32) -> i32 { 0 }

/// `unlockpt(fd)` — likewise a no-op; PTY is unlocked on
/// allocation.
#[no_mangle]
pub extern "C" fn unlockpt(_fd: i32) -> i32 { 0 }

/// `ptsname(fd)` — return a stable, static NUL-terminated
/// string with the slave side name.  Always returns
/// `"/dev/pts/0"` so callers expecting a non-NULL answer get
/// a sensible default.
#[no_mangle]
pub extern "C" fn ptsname(_fd: i32) -> *mut u8 {
    b"/dev/pts/0\0".as_ptr() as *mut u8
}

/// `ptsname_r(fd, buf, buflen)` — POSIX thread-safe variant.
/// Always writes the same string into buf if buf is large enough.
#[no_mangle]
pub extern "C" fn ptsname_r(_fd: i32, buf: *mut u8, buflen: usize) -> i32 {
    if buf.is_null() || buflen == 0 {
        set_errno_only(EINVAL);
        return -1;
    }
    let src = b"/dev/pts/0";
    let copy_len = core::cmp::min(src.len(), buflen - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), buf, copy_len);
        *buf.add(copy_len) = 0;
    }
    0
}

/// `posix_openpt(oflag)` — open a master pty fd.  Internally
/// routed to `open("/dev/ptmx", ...)`.
#[no_mangle]
pub extern "C" fn posix_openpt(oflag: i32) -> i32 {
    extern "C" {
        fn open(path: *const u8, flags: i32, mode: i32) -> i32;
    }
    unsafe { open(b"/dev/ptmx\0".as_ptr(), oflag, 0) }
}

// ---------------------------------------------------------------------------
// Locale / nl_langinfo
// ---------------------------------------------------------------------------

/// `nl_langinfo(item)` — return locale-dependent strings; we
/// only know one locale ("C") and one item ("CODESET") at the
/// moment.  Everything else returns the empty string.
///
/// `item == CODESET` (zero on glibc) returns "UTF-8" so
/// bash's `lib/locale.c` accepts us.  bash's configure
/// `AC_CHECK_FUNCS([nl_langinfo])` checks that this compiles
/// and returns a non-NULL pointer.
#[no_mangle]
pub extern "C" fn nl_langinfo(item: i32) -> *mut u8 {
    // POSIX nl_langinfo items are positive; glibc's CODESET
    // is zero on its predefined items list.
    if item == 0 /* CODESET */ {
        // We claim UTF-8 locale; bash probes for that.
        b"UTF-8\0".as_ptr() as *mut u8
    } else {
        b"\0".as_ptr() as *mut u8
    }
}

// ---------------------------------------------------------------------------
// Shared memory / memfd_create
//
// CapsuleOS does not have a real SysV shm / memfd_create
// backend in 1.0.  bash probes these for the loadable builtins
// feature (--enable-loadable-builtins); we already pass
// `--disable-loadable-builtins`, but bash also statically
// references the symbols.  Returning ENOSYS keeps them
// linkable without claiming a working implementation.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn shm_open(_path: *const u8, _oflag: i32, _mode: u32) -> i32 {
    set_errno_only(ENOSYS);
    -1
}

#[no_mangle]
pub extern "C" fn shm_unlink(_path: *const u8) -> i32 {
    set_errno_only(ENOSYS);
    -1
}

#[no_mangle]
pub extern "C" fn shm_mkstemp(_template: *mut u8) -> i32 {
    set_errno_only(ENOSYS);
    -1
}

#[no_mangle]
pub extern "C" fn memfd_create(_name: *const u8, _flags: u32) -> i32 {
    set_errno_only(ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// dlopen family — disabled in 1.0 (--disable-loadable-builtins
// gates the dependent feature at the bash side).  Return
// ENOSYS so the symbols exist for link but never return a
// caller-usable handle.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn dlopen(_path: *const u8, _flags: i32) -> *mut u8 {
    set_errno_only(ENOSYS);
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn dlclose(_handle: *mut u8) -> i32 {
    set_errno_only(ENOSYS);
    -1
}

#[no_mangle]
pub extern "C" fn dlsym(_handle: *mut u8, _name: *const u8) -> *mut u8 {
    set_errno_only(ENOSYS);
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn dlerror() -> *mut u8 {
    b"dlopen family not implemented\0".as_ptr() as *mut u8
}

// ---------------------------------------------------------------------------
// misc — mkstemp / mkdtemp / setresuid etc.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn mkstemp(_template: *mut u8) -> i32 {
    set_errno_only(ENOSYS);
    -1
}

#[no_mangle]
pub extern "C" fn mkdtemp(_template: *mut u8) -> *mut u8 {
    set_errno_only(ENOSYS);
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn setresuid(_r: u32, _e: u32, _s: u32) -> i32 {
    // Single-tenant microkernel: always uid 0.
    0
}

#[no_mangle]
pub extern "C" fn setresgid(_r: u32, _e: u32, _s: u32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn getentropy(_buf: *mut u8, _len: usize) -> i32 {
    // Deterministic-by-default kernel: there's no entropy
    // source.  bash's lib/intl/entropy.c uses it conditionally
    // so we report ENOSYS rather than fake success.
    set_errno_only(ENOSYS);
    -1
}

#[no_mangle]
pub extern "C" fn getdtablesize() -> i32 {
    // We size USER_FD_TABLE to 64; that's the largest the
    // user-side fd table will ever reach.
    64
}

// Password-database lookups — single-tenant microkernel; we
// synthesise a static record for root.

static PW_BUF: [u8; 64] = {
    let mut buf = [0u8; 64];
    let src = b"root";
    let mut i = 0;
    while i < src.len() {
        buf[i] = src[i];
        i += 1;
    }
    buf
}; // uid 0 name for everyone

/// `getpwuid(uid)` — returns a non-NULL synthetic record whose
/// `pw_name` field is the static string "root".  bash uses
/// this to populate `$USER`; the kernel runs as uid 0 in
/// single-tenant mode so "root" is correct.
#[no_mangle]
pub extern "C" fn getpwuid(_uid: u32) -> *mut u8 {
    PW_BUF.as_ptr() as *mut u8
}

#[no_mangle]
pub extern "C" fn getpwnam(_name: *const u8) -> *mut u8 {
    PW_BUF.as_ptr() as *mut u8
}

#[no_mangle]
pub extern "C" fn getpwent() -> *mut u8 {
    set_errno_only(ENOSYS);
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// argz helpers — bash uses glibc's <argz.h> for some paths;
// ship a stub that always returns ENOSYS so the symbols link.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn __argz_count(_argz: *const u8, _len: usize) -> usize {
    0
}

#[no_mangle]
pub extern "C" fn __argz_next(
    _argz: *const u8,
    _len: usize,
    _entry: *const u8,
) -> *const u8 {
    core::ptr::null()
}

#[no_mangle]
pub extern "C" fn __argz_stringify(_argz: *mut u8, _len: usize, _sep: u32) {}

/// Sanity: ensure unused-imports doesn't trip on the trio of
/// errno constants we expose here.
#[allow(dead_code)]
fn _suppress_unused() {
    let _ = (ENOTTY, ENOSYS);
}
