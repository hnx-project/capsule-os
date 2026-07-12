#![no_std]

pub mod syscalls;
pub use shared::status::Status;
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

#[derive(Debug, Clone, Copy)]
#[repr(C)]
enum FileAgentCmd {
    Open {
        path: [u8; 128],
        path_len: u32,
        flags: u32,
    },
    Close {
        fd: u32,
    },
    Read {
        fd: u32,
        len: usize,
    },
    Write {
        fd: u32,
        len: usize,
        vmo_handle: u32,
    },
    MkDir {
        path: [u8; 128],
        path_len: u32,
    },
    RmDir {
        path: [u8; 128],
        path_len: u32,
    },
    Unlink {
        path: [u8; 128],
        path_len: u32,
    },
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

    // 1. Resolve the path length.  We pass the user pointer straight
    //    through to the kernel; the kernel walks the page table and
    //    length-bounds the read on its side via Phase 6.3 K1.
    let mut len = 0;
    unsafe {
        while *path.add(len) != 0 && len < 127 {
            len += 1;
        }
    }

    // 2. Hand off to the kernel POSIX forwarder (P1).  The kernel
    //    talks to fileagent on our behalf and parks the resulting
    //    (process_id, fd) -> {session_chan, remote_fd} mapping in
    //    its PosixFdTable; we just get the local fd back.
    syscall!(
        SYSCALL_OPEN,
        path as usize,
        len,
        flags as usize,
        0,
        0,
        0
    ) as i32
}

#[no_mangle]
pub extern "C" fn read(fd: i32, buf: *mut u8, count: usize) -> isize {
    // All fds go straight through to the kernel.  fd=0 is the UART
    // stdin path (K-D2); fd>=3 is the fileagent forwarder (P2).
    syscall!(SYSCALL_READ, fd as usize, buf as usize, count, 0, 0, 0) as isize
}

#[no_mangle]
pub extern "C" fn write(fd: i32, buf: *const u8, count: usize) -> isize {
    // fd=1/2 are UART stdout/stderr (K-D2); fd>=3 is the fileagent
    // forwarder (P3).  See the file-header comment for the VMO
    // gap that Phase 6.6 will close for the no-VMO write path.
    syscall!(SYSCALL_WRITE, fd as usize, buf as usize, count, 0, 0, 0) as isize
}

#[no_mangle]
pub extern "C" fn close(fd: i32) -> i32 {
    // Kernel owns the POSIX fd table (P7) and closes the fileagent
    // session when the local fd is freed (P4).
    syscall!(SYSCALL_CLOSE, fd as usize, 0, 0, 0, 0, 0) as i32
}

#[no_mangle]
pub extern "C" fn exit(status: i32) -> ! {
    syscall!(SYSCALL_EXIT, status as usize, 0, 0, 0, 0, 0);
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
    syscall!(SYSCALL_GET_PID, 0, 0, 0, 0, 0, 0) as i32
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

fn send_dir_command(cmd: &FileAgentCmd) -> i32 {
    let session_chan = match syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => ch,
        Err(_) => return -1,
    };

    let cmd_slice = unsafe {
        core::slice::from_raw_parts(
            cmd as *const FileAgentCmd as *const u8,
            core::mem::size_of::<FileAgentCmd>(),
        )
    };
    if let Err(_) = syscalls::channel_write(session_chan, cmd_slice, &[]) {
        let _ = syscalls::close(session_chan);
        return -1;
    }

    let mut resp_buf = [0u8; 8];
    let mut resp_handles = [0u32; 2];
    let result = match syscalls::channel_read(session_chan, &mut resp_buf, &mut resp_handles) {
        Ok(read_len) if read_len >= 8 => unsafe {
            core::ptr::read_unaligned(resp_buf.as_ptr() as *const i64)
        },
        _ => -1,
    };

    let _ = syscalls::close(session_chan);
    result as i32
}

fn build_path_cmd(path: *const u8, kind: u8) -> Option<FileAgentCmd> {
    if path.is_null() {
        return None;
    }
    let mut len = 0;
    while len < 128 {
        let b = unsafe { *path.add(len) };
        if b == 0 {
            break;
        }
        len += 1;
    }
    let mut path_buf = [0u8; 128];
    Some(match kind {
        0 => FileAgentCmd::MkDir {
            path: path_buf,
            path_len: len as u32,
        },
        1 => FileAgentCmd::RmDir {
            path: path_buf,
            path_len: len as u32,
        },
        _ => FileAgentCmd::Unlink {
            path: path_buf,
            path_len: len as u32,
        },
    })
}

#[no_mangle]
pub extern "C" fn mkdir(path: *const u8) -> i32 {
    match build_path_cmd(path, 0) {
        Some(cmd) => send_dir_command(&cmd),
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn rmdir(path: *const u8) -> i32 {
    match build_path_cmd(path, 1) {
        Some(cmd) => send_dir_command(&cmd),
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn unlink(path: *const u8) -> i32 {
    match build_path_cmd(path, 2) {
        Some(cmd) => send_dir_command(&cmd),
        None => -1,
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
