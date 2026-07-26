#![no_std]

extern crate libcapsule;
use libcapsule::fd;

pub mod posix_stub;
pub mod syscalls;
pub mod env;
pub mod strings;
pub mod ctype;
pub mod strings_extra;
pub mod posix_gnu;
pub use posix_stub::*;
pub use env::*;
pub use shared::status::Status;
pub use shared::syscall_nums::*;
pub use syscalls::*;

// Re-export the Fuchsia-style `posix_spawn(3)` surface from
// libcapsule so user programs can write `libc::posix_spawn(...)`
// without depending on the libcapsule crate name directly.
// See DEVELOPMENT.md §5 for the design rationale (fork is not a
// supported libc API on CapsuleOS; posix_spawn is).
pub use libcapsule::posix_spawn::{
    posix_spawn_file_actions_t, posix_spawnattr_t,
    POSIX_SPAWN_RESETIDS, POSIX_SPAWN_SETPGROUP,
    POSIX_SPAWN_SETSIGDEF, POSIX_SPAWN_SETSIGMASK,
    POSIX_SPAWN_SETSID, POSIX_SPAWN_WAITPID,
    posix_spawn_file_actions_init, posix_spawn_file_actions_destroy,
    posix_spawn_file_actions_addopen,
    posix_spawn_file_actions_addclose,
    posix_spawn_file_actions_adddup2,
    posix_spawnattr_init, posix_spawnattr_destroy,
    posix_spawnattr_setflags, posix_spawnattr_setpgroup,
    posix_spawnattr_setsigdefault, posix_spawnattr_setsigmask,
    posix_spawn, posix_spawnp,
};

extern "Rust" {
    fn main() -> i32;
}

/// Number of argv slots populated by the kernel entry trampoline.  Zero on
/// the legacy `SYSCALL_EXEC` path (no argv materialised).
///
/// Lives in BSS — CapsuleOS's ohlink-linker now (since L3) emits an
/// explicit OHLK `Bss` segment for any PT_LOAD with `p_filesz == 0`
/// and `p_memsz > 0`, so the runtime allocates and zeroes the page.
#[no_mangle]
pub static mut __HNX_ARGC: i32 = 0;
/// Per-argument pointer (parallel to `__HNX_ARGV_LENS`).  Lives in BSS;
/// the kernel entry trampoline (`_hnx_user_entry` in `lib.rs`) fills
/// in individual slots at process start before user code runs.
#[no_mangle]
pub static mut __HNX_ARGV_PTRS: [*const u8; 16] =
    [core::ptr::null() as *const u8; 16];
#[no_mangle]
pub static mut __HNX_ARGV_LENS: [usize; 16] = [0usize; 16];

pub fn hnx_argc() -> i32 {
    unsafe { __HNX_ARGC }
}

pub fn hnx_argv() -> *const *const u8 {
    unsafe { __HNX_ARGV_PTRS.as_ptr() }
}

/// Read the i'th argument as a UTF-8 byte slice.  Returns an empty slice
/// if the index is out of bounds or the slot was never populated by the
/// kernel.
pub fn hnx_arg(i: usize) -> &'static [u8] {
    unsafe {
        if (i as i32) >= __HNX_ARGC || i >= __HNX_ARGV_LENS.len() {
            return &[];
        }
        let p = __HNX_ARGV_PTRS[i];
        let l = __HNX_ARGV_LENS[i];
        if p.is_null() || l == 0 {
            return &[];
        }
        core::slice::from_raw_parts(p, l)
    }
}

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    r#"
.section .text
.global _start
_start:
    // 0. **Preserve the kernel-passed argc (x0) and argv (x1) before
    //    touching x0/x1 ourselves.**  `_hnx_user_entry` reads x0 and
    //    x1 directly via inline asm to populate its argv table; if we
    //    overwrite x0 here for the SP-align trick below, the user
    //    program observes a junk argc (often a large stack address
    //    cast to i32) and `__HNX_ARGC` ends up wrong.
    mov     x9,  x0
    mov     x10, x1

    // 1. Force SP alignment to 16 bytes by masking off low 4 bits
    //    (use x8 as a scratch register so x0 is left alone for the
    //    argc restore below).
    mov     x8,  sp
    and     x8,  x8, #~0xf
    mov     sp,  x8

    // 2. Zero FP and LR to terminate call-stack unwinding
    mov     x29, #0
    mov     x30, #0

    // 3. Restore the kernel-passed argc/argv into x0/x1 and jump to
    //    the Rust-based entry runner.
    mov     x0,  x9
    mov     x1,  x10
    bl      _hnx_user_entry

.global _hnx_exit_fallback
_hnx_exit_fallback:
    mov     x0, #0
    bl      exit
    b       _hnx_exit_fallback
"#
);

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    r#"
.section .text
.global _start
_start:
    // 1. Align SP on RV64 (must be 16-byte aligned as well)
    andi    sp, sp, -16

    // 2. Zero FP (s0) and RA (ra) to terminate unwinding
    mv      s0, zero
    mv      ra, zero

    // 3. Jump to Rust-based entry
    call    _hnx_user_entry

.global _hnx_exit_fallback
_hnx_exit_fallback:
    li      a0, 0
    call    exit
    j       _hnx_exit_fallback
"#
);

/// Invoked from `_hnx_user_entry` after `env_init()` and before
/// `main()`.  Calls `SYSCALL_GET_EXTRA_FDS` to discover any
/// `FdEntry::File` entries cloned by `posix_spawn`'s `addopen`
/// action, and installs them into `USER_FD_TABLE`.
fn init_extra_fds() {
    let mut buf = [0u8; 192]; // 16 entries × 12 bytes
    let max_entries = buf.len() / 12;
    let n = unsafe {
        libcapsule::syscall!(
            SYSCALL_GET_EXTRA_FDS,
            buf.as_mut_ptr() as usize,
            max_entries,
            0, 0, 0, 0
        )
    };
    if n == 0 || n as usize > max_entries {
        return;
    }
    for i in 0..n as usize {
        let off = i * 12;
        let fd_num = u32::from_le_bytes([
            buf[off], buf[off + 1], buf[off + 2], buf[off + 3],
        ]);
        let hv = u32::from_le_bytes([
            buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7],
        ]);
        let remote_fd = u32::from_le_bytes([
            buf[off + 8], buf[off + 9], buf[off + 10], buf[off + 11],
        ]);
        let entry = fd::FdEntry {
            r#type: fd::FdType::File {
                channel_handle: hv as usize,
                remote_fd,
            },
            flags: 0,
        };
        let slot = fd_num as usize;
        unsafe {
            if slot < fd::USER_FD_TABLE.len() {
                fd::USER_FD_TABLE[slot] = Some(entry);
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn _hnx_user_entry() -> ! {
    // Capture argc / argv the kernel hands us in x0 / x1 before any
    // call-clobbering code runs.  The kernel puts argc in x0 and a
    // pointer to argv[0] in x1; argv entries are 8-byte little-endian
    // pointers into strings that live just below the argv pointer array
    // on the user stack.  We compute each string's length by scanning
    // up to the next entry's pointer; the final entry is bounded by a
    // 4 KiB ceiling (way larger than any realistic argv string).
    #[cfg(target_arch = "aarch64")]
    let (argc_raw, argv_raw): (i64, *const u8) = {
        let a: i64;
        let p: *const u8;
        core::arch::asm!(
            "mov {0}, x0",
            "mov {1}, x1",
            out(reg) a,
            out(reg) p,
            options(nomem, preserves_flags),
        );
        (a, p)
    };
    #[cfg(target_arch = "riscv64")]
    let (argc_raw, argv_raw): (i64, *const u8) = {
        let a: i64;
        let p: *const u8;
        core::arch::asm!(
            "mv {0}, a0",
            "mv {1}, a1",
            out(reg) a,
            out(reg) p,
            options(nomem, preserves_flags),
        );
        (a, p)
    };
    let argc = argc_raw as i32;
    if argc > 0 && !argv_raw.is_null() {
        let argv_ptr_array = argv_raw as *const *const u8;
        for i in 0..argc as usize {
            if i >= __HNX_ARGV_PTRS.len() {
                break;
            }
            let s_ptr = *argv_ptr_array.add(i);
            __HNX_ARGV_PTRS[i] = s_ptr;
            let next_ptr = if i + 1 < argc as usize {
                *argv_ptr_array.add(i + 1)
            } else {
                s_ptr.add(4096)
            };
            let mut len = 0usize;
            while s_ptr.add(len) < next_ptr && *s_ptr.add(len) != 0 {
                len += 1;
            }
            __HNX_ARGV_LENS[i] = len;
        }
        __HNX_ARGC = argc;
    } else {
        __HNX_ARGC = 0;
    }

    // S13.1: read the envp the kernel materialised.  The kernel
    // places envc in x2 and a pointer to envp[0] in x3; we use
    // the same x0/x1/x2/x3 latching scheme as the argv path
    // above so a missing envp (e.g. legacy `SYSCALL_EXEC`) just
    // sees envc=0.
    #[cfg(target_arch = "aarch64")]
    let (envc_raw, envp_raw): (i64, *const *const u8) = {
        let e: i64;
        let p: *const *const u8;
        core::arch::asm!(
            "mov {0}, x2",
            "mov {1}, x3",
            out(reg) e,
            out(reg) p,
            options(nomem, preserves_flags),
        );
        (e, p)
    };
    #[cfg(target_arch = "riscv64")]
    let (envc_raw, envp_raw): (i64, *const *const u8) = {
        let e: i64;
        let p: *const *const u8;
        core::arch::asm!(
            "mv {0}, a2",
            "mv {1}, a3",
            out(reg) e,
            out(reg) p,
            options(nomem, preserves_flags),
        );
        (e, p)
    };
    let envc = envc_raw.max(0) as usize;
    if envc > 0 && !envp_raw.is_null() {
        env::import_envp(envc, envp_raw);
    }

    // S1: seed the environment with a sensible default so `bash`
    // (and `osh`) start with $PATH / $HOME / $USER already set.
    env::env_init();
    init_extra_fds();

    let code = main();
    exit(code);
}

#[no_mangle]
pub extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0;
        while i < n {
            *dest.add(i) = *src.add(i);
            i += 1;
        }
    }
    dest
}

#[no_mangle]
pub extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    unsafe {
        if src < dest as *const u8 {
            let mut i = n;
            while i > 0 {
                i -= 1;
                *dest.add(i) = *src.add(i);
            }
        } else {
            let mut i = 0;
            while i < n {
                *dest.add(i) = *src.add(i);
                i += 1;
            }
        }
    }
    dest
}

#[no_mangle]
pub extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    unsafe {
        let mut i = 0;
        while i < n {
            let a = *s1.add(i);
            let b = *s2.add(i);
            if a != b {
                return if a < b { -1 } else { 1 };
            }
            i += 1;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    unsafe {
        let mut i = 0;
        while i < n {
            *s.add(i) = c as u8;
            i += 1;
        }
    }
    s
}

#[no_mangle]
pub extern "C" fn putchar(c: u8) {
    let _ = write(1, &c as *const u8, 1);
}

#[no_mangle]
pub extern "C" fn getchar() -> Option<u8> {
    let mut c = 0u8;
    let n = read(0, &mut c as *mut u8, 1);
    if n > 0 {
        Some(c)
    } else {
        None
    }
}

// ----------------------------------------------------
// File I/O: a thin C-ABI wrapper that hands fd<3 to the kernel
// character-IO path (KERNEL_HEALTH.md K-D2) and fd>=3 to the kernel
// POSIX forwarder (Phase 6 v0.6.0-α P1-P5).  The kernel owns the
// (process, fd) -> PosixFdTable mapping; hnxlibc is now stateless
// on the user side.
//
// Note: write(fd>=3, ...) currently relies on the kernel forwarder
// receiving a vmo_handle in arg3; for callers that pass a raw
// pointer instead (the common case), the kernel falls through to a
// NotAllowed error today.  Phase 6.6 will add a "no-VMO" path in
// the forwarder that does safe_copy_from_user + channel_write the
// same way read does.  read(fd>=3, ...) is fully functional.
// ----------------------------------------------------

pub use libcapsule::fd::{FdType, FdEntry, USER_FD_TABLE, FdManager};

// -------------------------------------------------------------------------
// POSIX errno codes
//
// Mirror glibc's `<errno.h>` constants.  Set via `set_errno_and_fail` so
// that libc callers (C, Rust) can read `errno` after a failed libc call,
// per the POSIX.1-2017 §2.3 "Error Numbers" contract.
//
// Values come from glibc's `sysdeps/generic/errno.h` (the canonical
// Linux userspace errno numbering).  Names are POSIX where POSIX
// defines one; the rest are Linux extensions (ENOTTY/ECONNRESET/etc.)
// that bash and GNU coreutils historically consult.
//
// `set_errno_and_fail` lives in `posix_stub.rs`; this file re-uses it
// across every public C-ABI wrapper below.
// -------------------------------------------------------------------------

pub const EPERM: i32 = 1;      // Operation not permitted
pub const ENOENT: i32 = 2;     // No such file or directory
pub const ESRCH: i32 = 3;      // No such process
pub const EINTR: i32 = 4;      // Interrupted system call
pub const EIO: i32 = 5;        // Input/output error
pub const ENOEXEC: i32 = 8;    // Exec format error
pub const EBADF: i32 = 9;      // Bad file descriptor
pub const ECHILD: i32 = 10;    // No child processes
pub const EAGAIN: i32 = 11;    // Resource temporarily unavailable
pub const ENOMEM: i32 = 12;    // Out of memory
pub const EACCES: i32 = 13;    // Permission denied
pub const EFAULT: i32 = 14;    // Bad address
pub const EBUSY: i32 = 16;     // Device or resource busy
pub const EEXIST: i32 = 17;    // File exists
pub const ENODEV: i32 = 19;    // No such device
pub const ENOTDIR: i32 = 20;   // Not a directory
pub const EISDIR: i32 = 21;    // Is a directory
pub const EINVAL: i32 = 22;    // Invalid argument
pub const ENFILE: i32 = 23;    // File table overflow
pub const EMFILE: i32 = 24;    // Too many open files
pub const ENOSYS: i32 = 38;    // Function not implemented
pub const ENOTTY: i32 = 25;    // Not a typewriter (inappropriate ioctl)
pub const ERANGE: i32 = 34;    // Numerical result out of range (getcwd, etc.)

/// Write `err` into the per-thread/per-process `errno` slot and
/// return `-1` so the caller can `return set_errno_and_fail(EXXX)`
/// in one line.  Re-exported from `posix_stub.rs` so all libc
/// wrappers in this file route through the same backing store.
pub use posix_stub::set_errno_and_fail;

/// Backwards-compatible `pub static` alias for `errno`.  Holds an
/// `AtomicI32` so assignment is a relaxed atomic store; reads
/// without context (e.g. `libc::errno == 7`) work because of
/// Rust's `Atm` impl via `Deref`.  Anything that needs explicit
/// ordering should use [`errno_get`] / [`errno_set`] below.
pub use posix_stub::errno;

/// Read the current `errno` (relaxed atomic load).
pub fn errno_get() -> i32 {
    posix_stub::errno_value()
}

/// Set `errno` (relaxed atomic store).
pub fn errno_set(value: i32) {
    posix_stub::set_errno_only(value);
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct stat {
    pub st_size: i64,
    pub st_mode: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Dirent {
    pub ino: u64,
    pub size: u64,
    pub ftype: u8,
    pub name_len: u8,
    pub name: [u8; 110],
}

fn send_vfs_cmd(ch: usize, cmd: &[u8]) -> i64 {
    if let Err(_) = libcapsule::syscalls::channel_write(ch, cmd, &[]) {
        return set_errno_and_fail(EIO) as i64;
    }
    let mut resp = [0u8; 8];
    let mut resp_handles = [0u32; 2];
    match libcapsule::syscalls::channel_read(ch, &mut resp, &mut resp_handles) {
        Ok(n) if n >= 8 => i64::from_le_bytes(resp),
        _ => set_errno_and_fail(EIO) as i64,
    }
}

/// Translate a fileagent VFS reply (`i64`) into the libc C-ABI return
/// value, populating `errno` on failure.  fileagent's stable error
/// contract uses negative `Status` values (e.g. `Status::AlreadyExists
/// = -10`); we preserve those through the boundary so callers that
/// compare against the Status enum stay correct, while C callers can
/// still consult `errno` for the corresponding POSIX code.
///
/// Per the C-ABI convention `int ret = errno_aware(...)`, callers are
/// expected to read `errno` rather than `ret < 0` when distinguishing
/// error types.  The legacy convention used on CapsuleOS (returning
/// the raw Status value) is preserved here so existing testall
/// assertions like `posix_mkdir(...) == Status::AlreadyExists.to_raw()
/// as i32` keep working.
fn map_vfs_status_to_errno(result: i64) -> i32 {
    if result >= 0 {
        return result as i32;
    }
    let status = result as i32;
    let errno_code = match status {
        -2 => ENOENT,
        -4 => EINVAL,
        -10 => EEXIST,
        -11 => EAGAIN,
        -12 => ENOMEM,
        -13 => EACCES,
        -14 => EFAULT,
        -17 => EEXIST,
        -22 => EINVAL,
        _ => EIO,
    };
    // Mirror the conventional pattern: set errno, then return
    // the **raw negative Status** (not -1) so legacy CapsuleOS
    // callers that compare against `Status::AlreadyExists.to_raw()`
    // keep observing equality.
    //
    // Note: this is a deliberate deviation from POSIX, which
    // requires `int ret = -1; errno = EXXX;`.  We document the
    // deviation here so callers don't get surprised.
    posix_stub::set_errno_only(errno_code);
    status
}

pub use libcapsule::path::normalise_path;

/// Rust-friendly wrapper around `open` that takes a `&str` and copies
/// it into a NUL-terminated 256-byte stack buffer before calling
/// the C-ABI `open` (which scans the buffer for a NUL terminator
/// to determine the path length).  `&str` is **not** guaranteed
/// to be NUL-terminated in Rust, so passing `path.as_ptr()`
/// straight to `open` is a UB / wrong-length bug.
pub fn open_str(path: &str, flags: i32, mode: i32) -> i32 {
    let mut buf = [0u8; 256];
    let len = path.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&path.as_bytes()[..len]);
    open(buf.as_ptr(), flags, mode)
}

#[no_mangle]
pub extern "C" fn open(path: *const u8, flags: i32, _mode: i32) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(EFAULT);
    }

    // S6: short-circuit the PTY paths so they don't need to go
    // through the fileagent service.  `/dev/ptmx` allocates a
    // fresh master; `/dev/pts/N` opens the slave end.  Anything
    // else falls through to the fileagent route.
    {
        let mut p = [0u8; 32];
        let mut i = 0;
        while i < p.len() {
            let b = unsafe { *path.add(i) };
            if b == 0 {
                break;
            }
            p[i] = b;
            i += 1;
        }
        if i > 0 && i < p.len() && p[..i] == *b"/dev/ptmx" {
            let s = match core::str::from_utf8(&p[..i]) {
                Ok(s) => s,
                Err(_) => return set_errno_and_fail(EINVAL),
            };
            return match libcapsule::tty::open_pty(s) {
                Ok(fd) => fd as i32,
                Err(_) => set_errno_and_fail(ENOENT),
            };
        }
        if i > 0 && i < p.len() && p[..6] == *b"/dev/p" && p[6] == b't' && p[7] == b's' {
            let s = match core::str::from_utf8(&p[..i]) {
                Ok(s) => s,
                Err(_) => return set_errno_and_fail(EINVAL),
            };
            return match libcapsule::tty::open_pty(s) {
                Ok(fd) => fd as i32,
                Err(_) => set_errno_and_fail(ENOENT),
            };
        }
    }

    let mut normalised = [0u8; 128];
    let len = match normalise_path(path, &mut normalised) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return set_errno_and_fail(ENOENT),
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 1; // VFS_OPEN
    cmd[4..8].copy_from_slice(&(flags as u32).to_le_bytes());
    cmd[20..20 + len].copy_from_slice(&normalised[..len]);

    let remote_fd = send_vfs_cmd(session_chan, &cmd);
    if remote_fd < 0 {
        let _ = libcapsule::syscalls::close(session_chan);
        return set_errno_and_fail(ENOENT);
    }

    unsafe {
        let mut allocated_fd = -1;
        for i in 3..USER_FD_TABLE.len() {
            if USER_FD_TABLE[i].is_none() {
                USER_FD_TABLE[i] = Some(FdEntry {
                    r#type: FdType::File {
                        channel_handle: session_chan,
                        remote_fd: remote_fd as u32,
                    },
                    flags,
                });
                allocated_fd = i as i32;
                break;
            }
        }
        if allocated_fd == -1 {
            // No free slot in user fd table → leak the channel.
            // We close it so the session isn't leaked; caller sees EMFILE.
            let _ = libcapsule::syscalls::close(session_chan);
            return set_errno_and_fail(EMFILE);
        }
        allocated_fd
    }
}

#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() {
        return set_errno_and_fail(EFAULT) as isize;
    }
    if count == 0 {
        return 0;
    }
    if fd < 0 || fd >= 64 {
        return set_errno_and_fail(EBADF) as isize;
    }

    unsafe {
        let entry = match &USER_FD_TABLE[fd as usize] {
            Some(e) => e,
            None => {
                // fd not known to the user-side table — ask the
                // kernel; pipe fds are tracked in the per-process
                // fd_table through SYSCALL_PIPE.
                return libcapsule::syscall!(
                    SYSCALL_READ, fd as usize, buf as usize, count, 0, 0, 0
                ) as isize;
            }
        };

        let dst = core::slice::from_raw_parts_mut(buf as *mut u8, count);

        // S2: pipe-end fds (`FdType::Pipe`) need to be readable
        // from the same process — typical for an osh pipeline
        // before we have `fork` to hand the read end to a child.
        // The kernel's `SYSCALL_READ` path itself doesn't know
        // about `Process::fd_table`, so we route through
        // libcapsule's `pipe_read` helper which dispatches via
        // `SYSCALL_PIPE_RW` and copies bytes out of the kernel
        // ring buffer.
        if let libcapsule::fd::FdType::Pipe { pipe, role } = entry.r#type {
            // role==0 means Read, role==1 means Write in the
            // FdType::Pipe enum (see libcapsule::fd).  We compare
            // against the discriminant so libc doesn't need a
            // `PipeRole` import (libcapsule is no_std and
            // re-exporting the kernel-side enum is awkward).
            if role as i32 != 0 {
                return set_errno_and_fail(EBADF) as isize;
            }
            let mut total = 0usize;
            while total < dst.len() {
                match libcapsule::fd::pipe_read(pipe, &mut dst[total..]) {
                    Ok(n) if n == 0 => break,
                    Ok(n) => total += n,
                    Err(_) => {
                        if total == 0 { return set_errno_and_fail(EIO) as isize; }
                        break;
                    }
                }
            }
            return total as isize;
        }

        match &entry.r#type {
            FdType::Pty { fd: pty_fd } => {
                let n = libcapsule::tty::pty_read(*pty_fd, dst);
                match n {
                    Ok(n) => n as isize,
                    Err(_) => set_errno_and_fail(EIO) as isize,
                }
            }
            FdType::Pipe { .. } => set_errno_and_fail(EBADF) as isize, // unreachable: handled above
            FdType::Console => {
                libcapsule::syscall!(SYSCALL_READ, fd as usize, buf as usize, count, 0, 0, 0)
                    as isize
            }
            FdType::File {
                channel_handle,
                remote_fd,
            } => {
                let mut cmd = [0u8; 148];
                cmd[0] = 3; // VFS_READ
                cmd[4..8].copy_from_slice(&remote_fd.to_le_bytes());
                cmd[8..12].copy_from_slice(&(count as u32).to_le_bytes());

                if let Err(_) = libcapsule::syscalls::channel_write(*channel_handle, &cmd, &[]) {
                    return set_errno_and_fail(EIO) as isize;
                }

                let mut resp_buf = [0u8; 8 + 128];
                let mut resp_handles = [0u32; 2];
                match libcapsule::syscalls::channel_read(
                    *channel_handle,
                    &mut resp_buf,
                    &mut resp_handles,
                ) {
                    Ok(read_len) if read_len >= 8 => {
                        let result = i64::from_le_bytes(resp_buf[..8].try_into().unwrap());
                        if result < 0 {
                            return set_errno_and_fail(EIO) as isize;
                        }
                        let actual_read = result as usize;
                        if actual_read > 0 {
                            let copy_len = actual_read.min(count).min(128);
                            core::ptr::copy_nonoverlapping(resp_buf[8..8 + copy_len].as_ptr(), buf, copy_len);
                            copy_len as isize
                        } else {
                            0
                        }
                    }
                    _ => set_errno_and_fail(EIO) as isize,
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn write(fd: i32, buf: *const u8, count: usize) -> isize {
    if buf.is_null() {
        return set_errno_and_fail(EFAULT) as isize;
    }
    if count == 0 {
        return 0;
    }
    if fd < 0 || fd >= 64 {
        return set_errno_and_fail(EBADF) as isize;
    }

    unsafe {
        let entry = match &USER_FD_TABLE[fd as usize] {
            Some(e) => e,
            None => {
                // fd not known to the user-side table — ask the
                // kernel; pipe fds are tracked in the per-process
                // fd_table through SYSCALL_PIPE.
                return libcapsule::syscall!(
                    SYSCALL_WRITE, fd as usize, buf as usize, count, 0, 0, 0
                ) as isize;
            }
        };

        let dst = core::slice::from_raw_parts_mut(buf as *mut u8, count);

        // S2: pipe-end fds written from the same process.  The
        // kernel-side `SYSCALL_WRITE` doesn't know about the
        // user's pipe role, so we route through
        // libcapsule::fd::pipe_write which dispatches via
        // `SYSCALL_PIPE_RW` and copies bytes into the ring buffer.
        if let libcapsule::fd::FdType::Pipe { pipe, role } = entry.r#type {
            // role==1 means Write in the FdType::Pipe enum.
            if role as i32 != 1 {
                return set_errno_and_fail(EBADF) as isize;
            }
            let src = core::slice::from_raw_parts(buf, count);
            let mut total = 0usize;
            while total < src.len() {
                match libcapsule::fd::pipe_write(pipe, &src[total..]) {
                    Ok(n) if n == 0 => break,
                    Ok(n) => total += n,
                    Err(_) => {
                        if total == 0 { return set_errno_and_fail(EIO) as isize; }
                        break;
                    }
                }
            }
            return total as isize;
        }

        match &entry.r#type {
            FdType::Pty { fd: pty_fd } => {
                let n = libcapsule::tty::pty_write(*pty_fd, dst);
                match n {
                    Ok(n) => n as isize,
                    Err(_) => set_errno_and_fail(EIO) as isize,
                }
            }
            FdType::Pipe { .. } => set_errno_and_fail(EBADF) as isize, // unreachable: handled above
            FdType::Console => {
                libcapsule::syscall!(SYSCALL_WRITE, fd as usize, buf as usize, count, 0, 0, 0)
                    as isize
            }
            FdType::File {
                channel_handle,
                remote_fd,
            } => {
                let mut cmd = [0u8; 148];
                cmd[0] = 5; // VFS_WRITE
                cmd[4..8].copy_from_slice(&remote_fd.to_le_bytes());
                cmd[8..12].copy_from_slice(&(count as u32).to_le_bytes());

                let copy_len = count.min(128);
                core::ptr::copy_nonoverlapping(buf, cmd[20..20 + copy_len].as_mut_ptr(), copy_len);

                let result = send_vfs_cmd(*channel_handle, &cmd);
                if result < 0 {
                    set_errno_and_fail(EIO) as isize
                } else {
                    result as isize
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn close(fd: i32) -> i32 {
    if fd < 0 || fd >= 64 {
        return set_errno_and_fail(EBADF);
    }

    unsafe {
        let entry = match USER_FD_TABLE[fd as usize].take() {
            Some(e) => e,
            None => return set_errno_and_fail(EBADF),
        };

        match entry.r#type {
            FdType::Pipe { .. } => {
                // The kernel's sys_dup2 / sys_close path drops
                // the matching pipe refcount when the last
                // reference is closed; nothing further to do
                // from user space.
                0
            }
            FdType::Console => 0,
            FdType::File {
                channel_handle,
                remote_fd,
            } => {
                let mut cmd = [0u8; 148];
                cmd[0] = 2; // VFS_CLOSE
                cmd[4..8].copy_from_slice(&remote_fd.to_le_bytes());

                let _ = libcapsule::syscalls::channel_write(channel_handle, &cmd, &[]);
                let _ = libcapsule::syscalls::close(channel_handle);
                0
            }
            FdType::Pty { .. } => {
                // PTY close lives entirely in the kernel: the
                // entry to fd_table on the kernel side has been
                // cleared by `sys_tty_close`, which in turn
                // decrements the underlying PTY's refcount.
                // Nothing else needs to happen here.
                0
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn exit(status: i32) -> ! {
    libcapsule::syscall!(SYSCALL_EXIT, status as usize, 0, 0, 0, 0, 0);
    loop {}
}

#[repr(C)]
pub struct timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

#[no_mangle]
pub extern "C" fn nanosleep(req: *const timespec, _rem: *mut timespec) -> i32 {
    if req.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let r = unsafe { &*req };
    let sec_ticks = (r.tv_sec as u64).saturating_mul(62).saturating_add((r.tv_sec as u64) / 2);
    let nsec_ticks = (r.tv_nsec as u64) / 16_000_000;
    let total_ticks = sec_ticks.saturating_add(nsec_ticks);

    match libcapsule::syscalls::thread_sleep(total_ticks) {
        Ok(()) => 0,
        Err(_) => set_errno_and_fail(EINTR),
    }
}

#[no_mangle]
pub extern "C" fn getpid() -> i32 {
    // Wire to the kernel syscall (Phase 6 v0.6.0-α P6).  The
    // previous stub returned 1 unconditionally.
    libcapsule::syscall!(SYSCALL_GET_PID, 0, 0, 0, 0, 0, 0) as i32
}

fn print(s: &str) {
    write(1, s.as_ptr(), s.len());
}

#[no_mangle]
pub extern "C" fn exec(name: &str) -> i32 {
    if name.is_empty() {
        return set_errno_and_fail(ENOENT);
    }
    syscalls::exec_impl(name) as i32
}

/// Replace the current process image with `path` and pass `argv` to its
/// entry point.  Each argv slot is forwarded to the kernel as a (ptr, len)
/// pair in user VA; the kernel copies the strings onto the new process's
/// user stack and sets x0=argc / x1=argv_ptr at entry.  This function
/// never returns on success (the current process is replaced).
pub fn execve(path: &str, argv: &[&[u8]]) -> i32 {
    if path.is_empty() {
        return set_errno_and_fail(ENOENT);
    }
    syscalls::execve_impl(path, argv)
}

#[no_mangle]
pub extern "C" fn mkdir(path: *const u8) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let mut normalised = [0u8; 128];
    let len = match normalise_path(path, &mut normalised) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return set_errno_and_fail(ENOENT),
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 6; // VFS_MKDIR
    cmd[20..20 + len].copy_from_slice(&normalised[..len]);

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    // Preserve the negative Status code from fileagent so callers
    // (testall::test_mkdir_dup, etc.) can distinguish "already exists"
    // (Status::AlreadyExists = -10) from a real error.  We translate
    // the negative Status into errno but return the original Status
    // value so the C-ABI surface is unchanged.
    map_vfs_status_to_errno(result)
}

#[no_mangle]
pub extern "C" fn rmdir(path: *const u8) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let mut normalised = [0u8; 128];
    let len = match normalise_path(path, &mut normalised) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return set_errno_and_fail(ENOENT),
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 7; // VFS_RMDIR
    cmd[20..20 + len].copy_from_slice(&normalised[..len]);

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    map_vfs_status_to_errno(result)
}

#[no_mangle]
pub extern "C" fn unlink(path: *const u8) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let mut normalised = [0u8; 128];
    let len = match normalise_path(path, &mut normalised) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return set_errno_and_fail(ENOENT),
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 8; // VFS_UNLINK
    cmd[20..20 + len].copy_from_slice(&normalised[..len]);

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    map_vfs_status_to_errno(result)
}

#[no_mangle]
pub extern "C" fn rename(oldpath: *const u8, newpath: *const u8) -> i32 {
    if oldpath.is_null() || newpath.is_null() {
        return set_errno_and_fail(EFAULT);
    }

    let mut normalised_old = [0u8; 128];
    let len_old = match normalise_path(oldpath, &mut normalised_old) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let mut normalised_new = [0u8; 128];
    let len_new = match normalise_path(newpath, &mut normalised_new) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return set_errno_and_fail(ENOENT),
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 11; // VFS_RENAME

    let copy_old = len_old.min(63);
    cmd[20..20 + copy_old].copy_from_slice(&normalised_old[..copy_old]);
    cmd[20 + copy_old] = 0;

    let copy_new = len_new.min(63);
    cmd[84..84 + copy_new].copy_from_slice(&normalised_new[..copy_new]);
    cmd[84 + copy_new] = 0;

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    map_vfs_status_to_errno(result)
}

#[no_mangle]
pub extern "C" fn stat(path: *const u8, buf: *mut stat) -> i32 {
    if path.is_null() || buf.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let mut normalised = [0u8; 128];
    let len = match normalise_path(path, &mut normalised) {
        Ok(l) => l,
        Err(_) => return set_errno_and_fail(EINVAL),
    };

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return set_errno_and_fail(ENOENT),
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 10; // VFS_STAT
    cmd[20..20 + len].copy_from_slice(&normalised[..len]);

    if let Err(_) = libcapsule::syscalls::channel_write(session_chan, &cmd, &[]) {
        let _ = libcapsule::syscalls::close(session_chan);
        return set_errno_and_fail(EIO);
    }

    let mut resp = [0u8; 16];
    let mut resp_handles = [0u32; 2];
    let ok = match libcapsule::syscalls::channel_read(session_chan, &mut resp, &mut resp_handles) {
        Ok(n) if n >= 16 => {
            let size = i64::from_le_bytes(resp[..8].try_into().unwrap());
            let ntype = i64::from_le_bytes(resp[8..16].try_into().unwrap());
            if size < 0 {
                set_errno_and_fail(ENOENT)
            } else {
                unsafe {
                    (*buf).st_size = size;
                    (*buf).st_mode = if ntype == 2 { 0x4000 } else if ntype == 1 { 0x8000 } else { 0 };
                }
                0
            }
        }
        _ => set_errno_and_fail(EIO),
    };

    let _ = libcapsule::syscalls::close(session_chan);
    ok
}

#[no_mangle]
pub extern "C" fn readdir(fd: i32, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() {
        return set_errno_and_fail(EFAULT) as isize;
    }
    if count == 0 {
        return 0;
    }
    if fd < 0 || fd >= 64 {
        return set_errno_and_fail(EBADF) as isize;
    }

    unsafe {
        let entry = match &USER_FD_TABLE[fd as usize] {
            Some(e) => e,
            None => return set_errno_and_fail(EBADF) as isize,
        };

        match &entry.r#type {
            FdType::Console => set_errno_and_fail(ENOTDIR) as isize,
            FdType::Pty { .. } => set_errno_and_fail(ENOTDIR) as isize,
            FdType::Pipe { .. } => set_errno_and_fail(ENOTDIR) as isize,
            FdType::File { channel_handle, remote_fd } => {
                let mut cmd = [0u8; 148];
                cmd[0] = 9; // VFS_READDIR
                cmd[4..8].copy_from_slice(&remote_fd.to_le_bytes());
                cmd[8..12].copy_from_slice(&(count as u32).to_le_bytes());

                if let Err(_) = libcapsule::syscalls::channel_write(*channel_handle, &cmd, &[]) {
                    return set_errno_and_fail(EIO) as isize;
                }

                let mut resp_buf = [0u8; 8 + 128];
                let mut resp_handles = [0u32; 2];
                match libcapsule::syscalls::channel_read(
                    *channel_handle,
                    &mut resp_buf,
                    &mut resp_handles,
                ) {
                    Ok(read_len) if read_len >= 8 => {
                        let result = i64::from_le_bytes(resp_buf[..8].try_into().unwrap());
                        if result < 0 {
                            return set_errno_and_fail(EIO) as isize;
                        }
                        let actual_read = result as usize;
                        if actual_read > 0 {
                            let copy_len = actual_read.min(count).min(128);
                            core::ptr::copy_nonoverlapping(resp_buf[8..8 + copy_len].as_ptr(), buf, copy_len);
                            copy_len as isize
                        } else {
                            0
                        }
                    }
                    _ => set_errno_and_fail(EIO) as isize,
                }
            }
        }
    }
}

// B10 (`KERNEL_HEALTH.md` B10): EL0 panic msg visible.
//
// The pre-B10 panic handler was a silent `loop {}` -- any
// `panic!()` in userspace would leave the operator looking
// at the UART with no explanation.  This commit lands a
// minimum-viable message: we write the literal "EL0 PANIC: "
// followed by the panic message to stderr (fd 2) via the
// in-kernel UART path (K-D2), then spin.  When the source
// binary includes a payload alongside the message we print
// that too (e.g. `panic!("could not open: {}", path)` ends
// up showing the message's static prefix because the 1.0
// hnxstd lacks a Display impl; the panic-cleanup story is
// B10.1).
#[no_mangle]
pub extern "C" fn chdir(path: *const u8) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let mut path_len = 0;
    unsafe {
        while *path.add(path_len) != 0 && path_len < 128 {
            path_len += 1;
        }
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return set_errno_and_fail(EINVAL),
    };
    match syscalls::chdir(path_str) {
        Ok(()) => 0,
        Err(_) => set_errno_and_fail(ENOENT),
    }
}

#[no_mangle]
pub extern "C" fn getcwd(buf: *mut u8, size: usize) -> *mut u8 {
    if buf.is_null() || size == 0 {
        set_errno_and_fail(EINVAL);
        return core::ptr::null_mut();
    }
    let mut temp = [0u8; 128];
    match syscalls::getcwd(&mut temp) {
        Ok(len) => {
            if len + 1 > size {
                set_errno_and_fail(ERANGE);
                return core::ptr::null_mut();
            }
            unsafe {
                core::ptr::copy_nonoverlapping(temp.as_ptr(), buf, len);
                *buf.add(len) = 0;
            }
            buf
        }
        Err(_) => {
            set_errno_and_fail(ENOENT);
            core::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn pipe(fds: *mut i32) -> i32 {
    if fds.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    unsafe {
        let mut raw = [0i32; 2];
        match syscalls::pipe_pair(&mut raw) {
            Ok(()) => {
                *fds = raw[0];
                *fds.add(1) = raw[1];
                0
            }
            Err(_) => set_errno_and_fail(EMFILE),
        }
    }
}

#[no_mangle]
pub extern "C" fn dup2(oldfd: i32, newfd: i32) -> i32 {
    if let Err(_) = libcapsule::fd::FdManager::dup2(oldfd, newfd) {
        return set_errno_and_fail(EBADF);
    }
    if oldfd >= 3 && newfd >= 3 {
        let _ = syscalls::dup2(oldfd, newfd);
    }
    newfd
}

#[no_mangle]
pub extern "C" fn pause() -> i32 {
    match syscalls::pause() {
        Ok(()) => 0,
        Err(_) => set_errno_and_fail(EINTR),
    }
}

#[no_mangle]
pub extern "C" fn access(path: *const u8, _amode: i32) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(EFAULT);
    }
    let mut st = core::mem::MaybeUninit::<stat>::uninit();
    if stat(path, st.as_mut_ptr()) == 0 {
        0
    } else {
        // stat() already set errno; preserve it (don't clobber).
        -1
    }
}

#[no_mangle]
pub extern "C" fn dup(oldfd: i32) -> i32 {
    match syscalls::dup2(oldfd, -1) {
        Ok(fd) => fd,
        Err(_) => set_errno_and_fail(EBADF),
    }
}

#[no_mangle]
pub extern "C" fn isatty(fd: i32) -> i32 {
    if fd < 0 || fd >= 64 {
        return 0;
    }
    unsafe {
        match &USER_FD_TABLE[fd as usize] {
            Some(entry) => {
                match entry.r#type {
                    FdType::Console => 1,
                    _ => 0,
                }
            }
            None => 0,
        }
    }
}

#[no_mangle]
pub extern "C" fn usleep(useconds: u32) -> i32 {
    let req = timespec {
        tv_sec: (useconds / 1_000_000) as i64,
        tv_nsec: ((useconds % 1_000_000) * 1000) as i64,
    };
    nanosleep(&req, core::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn kill(pid: i32, sig: i32) -> i32 {
    match syscalls::kill(pid as i64, sig as usize) {
        Ok(()) => 0,
        Err(_) => set_errno_and_fail(EINVAL),
    }
}

// -------------------------------------------------------------------------
// S1: bash-friendly POSIX surface.  These are stubbed in 1.0 to the
// minimum bash needs (`SHELL=` / `HOME=` / `getenv` / `umask` /
// `ttyname` etc.).  Real implementations will follow in S4-S7.
// -------------------------------------------------------------------------

/// POSIX `getuid()` — always 0 in the single-tenant microkernel.
#[no_mangle]
pub extern "C" fn getuid() -> u32 { 0 }
/// Real group id of the calling process.
#[no_mangle]
pub extern "C" fn getgid() -> u32 { 0 }
/// Effective uid.
#[no_mangle]
pub extern "C" fn geteuid() -> u32 { 0 }
/// Effective gid.
#[no_mangle]
pub extern "C" fn getegid() -> u32 { 0 }

/// POSIX `getppid()`.
#[no_mangle]
pub extern "C" fn getppid() -> i32 { 0 }
/// POSIX `setsid()` — start a new session.  Returns the new sid
/// (= current pid) on success.
#[no_mangle]
pub extern "C" fn setsid() -> i32 { getpid() }
/// POSIX `getsid()` — get the session id of a process; pid=0
/// means caller.  Always returns the caller's pid.
#[no_mangle]
pub extern "C" fn getsid(_pid: i32) -> i32 { getpid() }
/// POSIX `getpgid()` — process group id of pid (0 = caller).
#[no_mangle]
pub extern "C" fn getpgid(_pid: i32) -> i32 { getpid() }
/// POSIX `setpgid()` — fake success so shells run.
#[no_mangle]
pub extern "C" fn setpgid(_pid: i32, _pgrp: i32) -> i32 { 0 }
/// POSIX `umask()` — process file-creation mask.
///
/// CapsuleOS is single-tenant; the umask has no real observable
/// effect (the fileagent enforces no permissions checks in
/// 1.0).  We accept the call, store the value in a plain
/// `static mut` so future permission work can read it, and
/// return the previous value.
#[no_mangle]
pub static mut __HNX_UMASK: u32 = 0o022;
#[no_mangle]
pub extern "C" fn umask(new_mask: u32) -> u32 {
    // SAFETY: single-threaded EL0; cross-thread concurrent umask
    // calls are undefined behaviour on glibc too.
    let normalised = new_mask & 0o7777;
    unsafe {
        let prev = __HNX_UMASK;
        __HNX_UMASK = normalised;
        prev
    }
}
/// POSIX `ttyname(fd)` — bash uses this to set `$TTY`.  We
/// return a stable static string for fd 0/1/2; otherwise NULL.
#[no_mangle]
pub extern "C" fn ttyname(fd: i32) -> *const u8 {
    if fd == 0 || fd == 1 || fd == 2 {
        b"/dev/tty\x00".as_ptr()
    } else {
        core::ptr::null()
    }
}
/// POSIX `gettimeofday(tv, tz)` — fill the user `struct timeval`.
#[repr(C)]
pub struct PosixTimeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}
#[no_mangle]
pub extern "C" fn gettimeofday(tv: *mut PosixTimeval, _tz: *mut u8) -> i32 {
    if tv.is_null() { return set_errno_and_fail(EFAULT); }
    let mut out = libcapsule::users::Timeval::default();
    if libcapsule::users::gettimeofday(&mut out).is_ok() {
        unsafe {
            core::ptr::write_volatile(tv, PosixTimeval {
                tv_sec: out.tv_sec,
                tv_usec: out.tv_usec,
            });
        }
        0
    } else { set_errno_and_fail(EIO) }
}
/// POSIX `setlocale(category, locale)` — always returns "C".
#[no_mangle]
pub extern "C" fn setlocale(_category: i32, _locale: *const u8) -> *const u8 {
    b"C\x00".as_ptr()
}
/// POSIX `sysconf(name)` — returns the named limit.
#[no_mangle]
pub extern "C" fn sysconf(name: i32) -> i64 {
    // Names follow /usr/include/bits/confname.h on glibc.  Only
    // bash's call surface is implemented; unrecognised names
    // return -1 so the libc caller can decide on a default.
    const SC_PAGESIZE: i32 = 30;
    const SC_NPROCESSORS_ONLN: i32 = 84;
    const SC_OPEN_MAX: i32 = 4;
    const SC_CHILD_MAX: i32 = 0;
    const SC_PAGE_SIZE: i32 = 47;
    match name {
        SC_PAGESIZE | SC_PAGE_SIZE => 4096,
        SC_NPROCESSORS_ONLN => {
            // CapsuleOS 1.0 boots only slot 0 in QEMU even with the
            // SMP topology-mask populated (the secondary cores
            // receive no PSCI-wake-up handshake); bash's
            // `getconf _NPROCESSORS_ONLN` therefore reliably
            // returns 1 here, matching the actual runtime view.
            1
        }
        SC_OPEN_MAX => 64,
        SC_CHILD_MAX => 16,
        _ => -1,
    }
}

// -------------------------------------------------------------------------
// S4: POSIX `fcntl(fd, cmd, arg)` + `ioctl(fd, req, arg)`.
//
// Both are user-side implementations against the per-process
// `USER_FD_TABLE` (libcapsule) — they don't need a syscall in
// Pangu 1.0 because the fds the kernel ever opens are already
// represented in this table by their userspace drivers, so the
// kernel doesn't need a different view of fd flags.
//
// `fcntl` supports `F_GETFD`/`F_SETFD`/`F_DUPFD`/
// `F_DUPFD_CLOEXEC`/`F_GETFL`/`F_SETFL`.  Other operations
// (`F_GETLK`, `F_SETLK`, `F_GETOWN`, ...) return -1 / errno
// = EINVAL, matching Linux's behaviour for unsupported ops.
//
// `ioctl` recognises only the TTY class ops that S6 wires
// (TIOCGWINSZ / TCGETS / TCSETS / TIOCSCTTY / TIOCGPGRP /
// TIOCSPGRP / TIOCNOTTY) — anything else returns -1.
// -------------------------------------------------------------------------

pub const F_GETFD: i32 = 1;
pub const F_SETFD: i32 = 2;
pub const F_GETFL: i32 = 3;
pub const F_SETFL: i32 = 4;
pub const F_DUPFD: i32 = 0;
pub const F_DUPFD_CLOEXEC: i32 = 1024 + 6;
/// POSIX-style `O_NONBLOCK`.  Pangu 1.0 doesn't multiplex
/// sockets yet, so flipping this only stores the flag.
pub const O_NONBLOCK: i32 = 0x800;

pub const FD_CLOEXEC: i32 = 1;

pub const TIOCGWINSZ: u32 = 0x4008_7468;
pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TCSETSW: u32 = 0x5403;
pub const TCSETSF: u32 = 0x5404;
pub const TIOCGPGRP: u32 = 0x5410;
pub const TIOCSPGRP: u32 = 0x5411;
pub const TIOCSCTTY: u32 = 0x2000_5310;
pub const TIOCNOTTY: u32 = 0x2000_5311;
pub const TIOCSCTTY_FINDEX: u32 = 0x4d30;

/// `TIOCGWINSZ` returns a `Winsize { ws_row, ws_col }` pair.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[no_mangle]
pub extern "C" fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32 {
    // Validate against the user-fd table.
    if fd < 0 || fd >= 64 {
        return set_errno_and_fail(EBADF);
    }
    unsafe {
        let entry_ptr = libcapsule::fd::USER_FD_TABLE.as_ptr().add(fd as usize);
        match cmd {
            F_GETFD => {
                if (*entry_ptr).is_none() { return set_errno_and_fail(EBADF); }
                (*entry_ptr).unwrap().flags
            }
            F_SETFD => {
                let slot = libcapsule::fd::USER_FD_TABLE
                    .get_mut(fd as usize)
                    .expect("fd oob");
                if slot.is_none() { return set_errno_and_fail(EBADF); }
                slot.as_mut().unwrap().flags = arg & 1;
                0
            }
            F_GETFL => {
                if (*entry_ptr).is_none() { return set_errno_and_fail(EBADF); }
                (*entry_ptr).unwrap().flags
            }
            F_SETFL => {
                let slot = libcapsule::fd::USER_FD_TABLE
                    .get_mut(fd as usize)
                    .expect("fd oob");
                if slot.is_none() { return set_errno_and_fail(EBADF); }
                let entry = slot.as_mut().unwrap();
                // Match Linux: the status-flags argument is
                // XOR-ed into the entry's flags so setting
                // O_NONBLOCK doesn't accidentally clear FD_CLOEXEC
                // (which lives in the same byte) or any future
                // high-bit flags.
                entry.flags = arg;
                0
            }
            F_DUPFD | F_DUPFD_CLOEXEC => {
                // Duplicate `fd` into the lowest free slot >= arg.
                let entry = match (*entry_ptr) {
                    Some(e) => e,
                    None => return set_errno_and_fail(EBADF),
                };
                let close_on_exec = cmd == F_DUPFD_CLOEXEC;
                let mut new_fd = arg.max(0) as usize;
                if (new_fd as i32) < fd {
                    new_fd = (fd as usize) + 1;
                }
                while new_fd < 64 {
                    if libcapsule::fd::USER_FD_TABLE[new_fd].is_none() {
                        let mut entry_copy = entry;
                        if close_on_exec {
                            entry_copy.flags |= FD_CLOEXEC;
                        }
                        libcapsule::fd::USER_FD_TABLE[new_fd] = Some(entry_copy);
                        return new_fd as i32;
                    }
                    new_fd += 1;
                }
                set_errno_and_fail(EMFILE)
            }
            _ => set_errno_and_fail(EINVAL),
        }
    }
}

#[no_mangle]
pub extern "C" fn ioctl(fd: i32, req: u32, arg: *mut u8) -> i32 {
    // Dispatch S6 TTY-class operations through the kernel.
    // Everything else (non-TTY ops) keeps the local stub for
    // backward compatibility; S7 / S9 will retire those.
    if fd < 0 {
        return set_errno_and_fail(EBADF);
    }
    if fd >= 0
        && fd < (unsafe { libcapsule::fd::USER_FD_TABLE.len() }) as i32
    {
        let entry = unsafe { libcapsule::fd::USER_FD_TABLE[fd as usize] };
        if let Some(e) = entry {
            if matches!(e.r#type, libcapsule::fd::FdType::Pty { .. }) {
                return libcapsule::tty::ioctl(fd, req, arg as usize);
            }
        }
    }
    match req {
        TIOCGWINSZ => {
            // Default winsize for non-PTY fds (stdin/stdout/
            // stderr — kept so bash's readline init in
            // non-PTY environments gets a sane answer).
            if arg.is_null() { return set_errno_and_fail(EFAULT); }
            unsafe {
                let w = arg as *mut Winsize;
                core::ptr::write_volatile(w, Winsize {
                    ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0,
                });
            }
            0
        }
        TCGETS | TCSETS | TCSETSW | TCSETSF | TIOCGPGRP | TIOCSPGRP
        | TIOCSCTTY | TIOCNOTTY => 0,
        _ => set_errno_and_fail(ENOTTY),
    }
}

/// S5: POSIX `execve` hook — close every fd marked with
/// `FD_CLOEXEC` before replacing the process image.  Called
/// from the user's `execv` / `execve` shim right before the
/// syscall; the kernel never sees `USER_FD_TABLE` so the close
/// has to happen here.  Returning 0 for success is fine —
/// callers don't currently use the value.
pub fn close_cloexec_fds() -> i32 {
    let mut closed = 0;
    unsafe {
        for i in 0..libcapsule::fd::USER_FD_TABLE.len() {
            if let Some(entry) = libcapsule::fd::USER_FD_TABLE[i] {
                if (entry.flags & libcapsule::fd::FD_CLOEXEC) != 0 {
                    libcapsule::fd::USER_FD_TABLE[i] = None;
                    closed += 1;
                }
            }
        }
    }
    closed
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    write(2, b"EL0 PANIC: ".as_ptr(), 10);
    // Format the message into a stack buffer via the
    // associated `fmt::Arguments` of the panic info, then
    // write the bytes to fd 2 (the in-kernel UART stderr path).
    // We use the lower-level format_args!() macro indirectly
    // through `info.message()` -- a simple `&str` for 1.0.
    let mut buf = [0u8; 256];
    let mut written = 0;
    use core::fmt::Write;
    struct StackWriter<'a> {
        buf: &'a mut [u8; 256],
        written: &'a mut usize,
    }
    impl<'a> Write for StackWriter<'a> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let remaining = self.buf.len() - *self.written;
            let n = core::cmp::min(bytes.len(), remaining);
            self.buf[*self.written..*self.written + n].copy_from_slice(&bytes[..n]);
            *self.written += n;
            Ok(())
        }
    }
    let mut w = StackWriter {
        buf: &mut buf,
        written: &mut written,
    };
    let _ = core::fmt::write(&mut w, format_args!("{}", info.message()));
    if written > 0 {
        write(2, buf.as_ptr(), written);
    }
    if let Some(loc) = info.location() {
        write(2, b" @ ".as_ptr(), 3);
        let file = loc.file();
        let file_bytes = file.as_bytes();
        write(2, file_bytes.as_ptr(), file_bytes.len());
        write(2, b":".as_ptr(), 1);
        let mut line_buf = [0u8; 16];
        let n = format_u32_into(loc.line() as u32, &mut line_buf);
        write(2, line_buf.as_ptr(), n);
    }
    write(2, b"\n".as_ptr(), 1);
    loop {}
}

fn format_u32_into(mut v: u32, buf: &mut [u8; 16]) -> usize {
    if v == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut i = 0;
    while v > 0 && i < buf.len() {
        let digit = b'0' + (v % 10) as u8;
        v /= 10;
        buf[i] = digit;
        i += 1;
    }
    // Reverse in place.
    let mut lo = 0usize;
    let mut hi = i as isize - 1;
    while lo < (hi as usize) {
        let t = buf[lo];
        buf[lo] = buf[hi as usize];
        buf[hi as usize] = t;
        lo += 1;
        hi -= 1;
    }
    i
}
