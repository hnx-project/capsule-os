#![no_std]

extern crate libcapsule;

pub mod posix_stub;
pub mod syscalls;
pub use shared::status::Status;
pub use shared::syscall_nums::*;
pub use syscalls::*;

extern "Rust" {
    fn main() -> i32;
}

/// Number of argv slots populated by the kernel entry trampoline.  Zero on
/// the legacy `SYSCALL_EXEC` path (no argv materialised).
#[no_mangle]
pub static mut __HNX_ARGC: i32 = 0;
/// Per-argument pointer (parallel to `__HNX_ARGV_LENS`).  Pre-filled with
/// non-zero sentinel values so the linker keeps these symbols in the
/// data segment of the OHLK image — CapsuleOS's ohlink-linker currently
/// drops PT_LOAD segments whose `p_filesz == 0`, so a zero-init `static
/// mut` would land in unmapped memory and corrupt the process at entry.
#[no_mangle]
pub static mut __HNX_ARGV_PTRS: [*const u8; 16] = [0xDEAD_BEEF as *const u8; 16];
#[no_mangle]
pub static mut __HNX_ARGV_LENS: [usize; 16] = [0xFFFF_FFFF_FFFF_FFFFusize; 16];

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FdType {
    Console,
    File {
        channel_handle: usize,
        remote_fd: u32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct FdEntry {
    pub r#type: FdType,
    pub flags: i32,
}

#[no_mangle]
pub static mut USER_FD_TABLE: [Option<FdEntry>; 64] = [
    Some(FdEntry {
        r#type: FdType::Console,
        flags: 0,
    }),
    Some(FdEntry {
        r#type: FdType::Console,
        flags: 1,
    }),
    Some(FdEntry {
        r#type: FdType::Console,
        flags: 2,
    }),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct stat {
    pub st_size: i64,
    pub st_mode: u32,
}

fn send_vfs_cmd(ch: usize, cmd: &[u8]) -> i64 {
    if let Err(_) = libcapsule::syscalls::channel_write(ch, cmd, &[]) {
        return -1;
    }
    let mut resp = [0u8; 8];
    let mut resp_handles = [0u32; 2];
    match libcapsule::syscalls::channel_read(ch, &mut resp, &mut resp_handles) {
        Ok(n) if n >= 8 => i64::from_le_bytes(resp),
        _ => -1,
    }
}

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
        return -1;
    }

    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 1; // VFS_OPEN
    cmd[4..8].copy_from_slice(&(flags as u32).to_le_bytes());
    unsafe {
        core::ptr::copy_nonoverlapping(path, cmd[20..20 + len].as_mut_ptr(), len);
    }

    let remote_fd = send_vfs_cmd(session_chan, &cmd);
    if remote_fd < 0 {
        let _ = libcapsule::syscalls::close(session_chan);
        return -1;
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
            let _ = libcapsule::syscalls::close(session_chan);
        }
        allocated_fd
    }
}

#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() || count == 0 {
        return 0;
    }
    if fd < 0 || fd >= 64 {
        return -1;
    }

    unsafe {
        let entry = match &USER_FD_TABLE[fd as usize] {
            Some(e) => e,
            None => return -1,
        };

        match &entry.r#type {
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
                    return -1;
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
                            return -1;
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
                    _ => -1,
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn write(fd: i32, buf: *const u8, count: usize) -> isize {
    if buf.is_null() || count == 0 {
        return 0;
    }
    if fd < 0 || fd >= 64 {
        return -1;
    }

    unsafe {
        let entry = match &USER_FD_TABLE[fd as usize] {
            Some(e) => e,
            None => return -1,
        };

        match &entry.r#type {
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
                    -1
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
        return -1;
    }

    unsafe {
        let entry = match USER_FD_TABLE[fd as usize].take() {
            Some(e) => e,
            None => return -1,
        };

        match entry.r#type {
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
        }
    }
}

#[no_mangle]
pub extern "C" fn exit(status: i32) -> ! {
    libcapsule::syscall!(SYSCALL_EXIT, status as usize, 0, 0, 0, 0, 0);
    loop {}
}

#[no_mangle]
pub extern "C" fn nanosleep(_req: *const u8, _rem: *mut u8) -> i32 {
    0
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
        return -1;
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
        return -1;
    }
    syscalls::execve_impl(path, argv)
}

#[no_mangle]
pub extern "C" fn mkdir(path: *const u8) -> i32 {
    if path.is_null() {
        return -1;
    }
    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 6; // VFS_MKDIR
    unsafe {
        core::ptr::copy_nonoverlapping(path, cmd[20..20 + len].as_mut_ptr(), len);
    }

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    result as i32
}

#[no_mangle]
pub extern "C" fn rmdir(path: *const u8) -> i32 {
    if path.is_null() {
        return -1;
    }
    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 7; // VFS_RMDIR
    unsafe {
        core::ptr::copy_nonoverlapping(path, cmd[20..20 + len].as_mut_ptr(), len);
    }

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    result as i32
}

#[no_mangle]
pub extern "C" fn unlink(path: *const u8) -> i32 {
    if path.is_null() {
        return -1;
    }
    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 8; // VFS_UNLINK
    unsafe {
        core::ptr::copy_nonoverlapping(path, cmd[20..20 + len].as_mut_ptr(), len);
    }

    let result = send_vfs_cmd(session_chan, &cmd);
    let _ = libcapsule::syscalls::close(session_chan);
    result as i32
}

#[no_mangle]
pub extern "C" fn stat(path: *const u8, buf: *mut stat) -> i32 {
    if path.is_null() || buf.is_null() {
        return -1;
    }
    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }

    let session_chan = match libcapsule::syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    let mut cmd = [0u8; 148];
    cmd[0] = 10; // VFS_STAT
    unsafe {
        core::ptr::copy_nonoverlapping(path, cmd[20..20 + len].as_mut_ptr(), len);
    }

    if let Err(_) = libcapsule::syscalls::channel_write(session_chan, &cmd, &[]) {
        let _ = libcapsule::syscalls::close(session_chan);
        return -1;
    }

    let mut resp = [0u8; 16];
    let mut resp_handles = [0u32; 2];
    let ok = match libcapsule::syscalls::channel_read(session_chan, &mut resp, &mut resp_handles) {
        Ok(n) if n >= 16 => {
            let size = i64::from_le_bytes(resp[..8].try_into().unwrap());
            let ntype = i64::from_le_bytes(resp[8..16].try_into().unwrap());
            unsafe {
                (*buf).st_size = size;
                (*buf).st_mode = if ntype == 2 { 0x4000 } else if ntype == 1 { 0x8000 } else { 0 };
            }
            0
        }
        _ => -1,
    };

    let _ = libcapsule::syscalls::close(session_chan);
    ok
}

#[no_mangle]
pub extern "C" fn readdir(fd: i32, buf: *mut u8, count: usize) -> isize {
    if buf.is_null() || count == 0 {
        return 0;
    }
    if fd < 0 || fd >= 64 {
        return -1;
    }

    unsafe {
        let entry = match &USER_FD_TABLE[fd as usize] {
            Some(e) => e,
            None => return -1,
        };

        match &entry.r#type {
            FdType::Console => -1,
            FdType::File {
                channel_handle,
                remote_fd,
            } => {
                let mut cmd = [0u8; 148];
                cmd[0] = 9; // VFS_READDIR
                cmd[4..8].copy_from_slice(&remote_fd.to_le_bytes());
                cmd[8..12].copy_from_slice(&(count as u32).to_le_bytes());

                if let Err(_) = libcapsule::syscalls::channel_write(*channel_handle, &cmd, &[]) {
                    return -1;
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
                            return -1;
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
                    _ => -1,
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
