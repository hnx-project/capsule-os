//! `strings` — POSIX/GNU `<string.h>` and `<strings.h>` extensions
//! that bash 5.3's readline / lib/intl probes at autoconf time.
//!
//! Every function here either:
//!   - runs entirely in user space (no syscall), or
//!   - returns a stable answer (errno = `ENOSYS`) when the kernel
//!     does not implement the underlying facility.
//!
//! Coverage targets (the bash 5.3 / readline 8.x probe list):
//!   strsignal, strcasecmp, strncasecmp, memchr, memrchr,
//!   strsep, strlcpy, strlcat.
//!
//! All exported symbols are declared `#[no_mangle] pub extern "C"`
//! so C callers can resolve them at link time.

#![allow(unused_imports)]

use core::cmp::Ordering;

/// POSIX `strcasecmp(s1, s2)` — case-insensitive byte compare
/// of two NUL-terminated strings.  Used by bash's locale.c,
/// readline's histexpand.c, and many lib/intl paths.
#[no_mangle]
pub extern "C" fn strcasecmp(s1: *const u8, s2: *const u8) -> i32 {
    if s1.is_null() || s2.is_null() {
        return if s1 == s2 { 0 } else if s1.is_null() { -1 } else { 1 };
    }
    let mut a = s1;
    let mut b = s2;
    loop {
        let ba = unsafe { *a };
        let bb = unsafe { *b };
        let la = if (ba as i32 >= b'A' as i32 && ba as i32 <= b'Z' as i32) {
            ba + (b'a' - b'A')
        } else {
            ba
        };
        let lb = if (bb as i32 >= b'A' as i32 && bb as i32 <= b'Z' as i32) {
            bb + (b'a' - b'A')
        } else {
            bb
        };
        match la.cmp(&lb) {
            Ordering::Equal if la == 0 => return 0,
            Ordering::Equal => {
                a = unsafe { a.add(1) };
                b = unsafe { b.add(1) };
            }
            Ordering::Less => return -1,
            Ordering::Greater => return 1,
        }
    }
}

/// POSIX `strncasecmp(s1, s2, n)` — case-insensitive byte compare
/// of at most `n` bytes.
#[no_mangle]
pub extern "C" fn strncasecmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    if n == 0 {
        return 0;
    }
    if s1.is_null() || s2.is_null() {
        return if s1 == s2 { 0 } else if s1.is_null() { -1 } else { 1 };
    }
    let mut a = s1;
    let mut b = s2;
    for _ in 0..n {
        let ba = unsafe { *a };
        let bb = unsafe { *b };
        let la = if (ba as i32 >= b'A' as i32 && ba as i32 <= b'Z' as i32) {
            ba + (b'a' - b'A')
        } else {
            ba
        };
        let lb = if (bb as i32 >= b'A' as i32 && bb as i32 <= b'Z' as i32) {
            bb + (b'a' - b'A')
        } else {
            bb
        };
        if la != lb {
            return if la < lb { -1 } else { 1 };
        }
        if la == 0 {
            return 0;
        }
        a = unsafe { a.add(1) };
        b = unsafe { b.add(1) };
    }
    0
}

/// BSD `strsep(stringp, delim)` — tokenise a NUL-terminated string.
///
/// `*stringp` points to a writable pointer; on entry it should
/// point to the current scan position, on exit it points just
/// past the token.  Multiple delimiters are accepted; the first
/// occurrence is replaced by `\0` and the previous pointer
/// returned.  Returns NULL when no field remains.
#[no_mangle]
pub extern "C" fn strsep(stringp: *mut *mut u8, delim: *const u8) -> *mut u8 {
    if stringp.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        if (*stringp).is_null() {
            return core::ptr::null_mut();
        }
        let s = *stringp;
        if delim.is_null() {
            *stringp = core::ptr::null_mut();
            return s;
        }
        let mut p = s;
        while *p != 0 {
            let mut d = delim;
            while *d != 0 {
                if *p == *d {
                    *p = 0;
                    let next = p.add(1);
                    *stringp = if *next != 0 { next } else { core::ptr::null_mut() };
                    return s;
                }
                d = d.add(1);
            }
            p = p.add(1);
        }
        *stringp = core::ptr::null_mut();
        s
    }
}

/// POSIX `memchr(s, c, n)` — locate the first occurrence of `c`
/// in the first `n` bytes of the uninitialised object pointed to
/// by `s`.  Returns a pointer to the byte or NULL.
#[no_mangle]
pub extern "C" fn memchr(s: *const u8, c: i32, n: usize) -> *const u8 {
    if s.is_null() || n == 0 {
        return core::ptr::null();
    }
    let needle = c as u8;
    unsafe {
        let mut p = s;
        for _ in 0..n {
            if *p == needle {
                return p;
            }
            p = p.add(1);
        }
        core::ptr::null()
    }
}

/// GNU `memrchr(s, c, n)` — like `memchr` but searches from the
/// end of the buffer backwards.
#[no_mangle]
pub extern "C" fn memrchr(s: *const u8, c: i32, n: usize) -> *const u8 {
    if s.is_null() || n == 0 {
        return core::ptr::null();
    }
    let needle = c as u8;
    unsafe {
        let mut p = s.add(n);
        let mut i = n;
        while i > 0 {
            p = p.sub(1);
            if *p == needle {
                return p;
            }
            i -= 1;
        }
        core::ptr::null()
    }
}

/// BSD `strlcpy(dst, src, size)` — bounded string copy that
/// always NUL-terminates when `size > 0`.  Returns the length of
/// the source string (excluding the NUL), so callers can detect
/// truncation by comparing the return value to `size - 1`.
///
/// Always present on BSD / macOS; some Linux distros also ship
/// it via `<bsd/string.h>`.  Bash's lib/sh/stringlib.c reaches
/// for it via a config probe.
#[no_mangle]
pub extern "C" fn strlcpy(dst: *mut u8, src: *const u8, size: usize) -> usize {
    if dst.is_null() || src.is_null() || size == 0 {
        return 0;
    }
    let mut src_len = 0usize;
    let mut i = 0usize;
    unsafe {
        while i + 1 < size {
            let b = *src.add(i);
            *dst.add(i) = b;
            if b == 0 {
                return src_len;
            }
            src_len += 1;
            i += 1;
        }
        // Ran out of space — NUL-terminate.
        *dst.add(size - 1) = 0;
    }
    // Continue scanning source to compute its true length, but
    // don't write anything more.  This is the canonical
    // strlcpy semantics ("returns strlen(src)").
    while unsafe { *src.add(i + 1) } != 0 {
        src_len += 1;
        i += 1;
    }
    src_len
}

/// BSD `strlcat(dst, src, size)` — bounded string concat that
/// always NUL-terminates when `size > 0`.  Returns the length
/// of the would-be concatenated string.
#[no_mangle]
pub extern "C" fn strlcat(dst: *mut u8, src: *const u8, size: usize) -> usize {
    if dst.is_null() || src.is_null() || size == 0 {
        return 0;
    }
    // Locate the existing NUL inside dst, bounded by size.
    let mut dst_len = 0usize;
    unsafe {
        while dst_len < size && *dst.add(dst_len) != 0 {
            dst_len += 1;
        }
    }
    let mut src_len = 0usize;
    let mut i = 0usize;
    unsafe {
        // Copy characters while there is space.
        while dst_len + 1 < size {
            let b = *src.add(i);
            if b == 0 {
                *dst.add(dst_len) = 0;
                return dst_len + src_len;
            }
            *dst.add(dst_len) = b;
            dst_len += 1;
            src_len += 1;
            i += 1;
        }
        // Out of space — NUL-terminate at size-1.
        if size > 0 {
            *dst.add(size - 1) = 0;
        }
        // Continue scanning source for the true length.
        while *src.add(i) != 0 {
            src_len += 1;
            i += 1;
        }
    }
    dst_len + src_len
}

/// POSIX `strsignal(sig)` — return a stable human-readable
/// string for the given signal number.  CapsuleOS has only a
/// 32-signal model so anything outside `[1, NSIG]` is reported
/// as "Unknown signal".
///
/// Returns a pointer to a static string.  Since the libc user
/// build is `#![no_std]` we can't allocate; every accepted
/// signal maps to a `&'static str` reference.  Strings are
/// long enough to be unambiguous in shell error messages
/// ("Interrupt", "Segmentation fault", etc.).
#[no_mangle]
pub extern "C" fn strsignal(sig: i32) -> *const u8 {
    let s = match sig {
        1 => "Hangup",
        2 => "Interrupt",
        3 => "Quit",
        4 => "Illegal instruction",
        5 => "Trace/breakpoint trap",
        6 => "Aborted",
        7 => "Bus error",
        8 => "Floating point exception",
        9 => "Killed",
        10 => "User defined signal 1",
        11 => "Segmentation fault",
        12 => "User defined signal 2",
        13 => "Broken pipe",
        14 => "Alarm clock",
        15 => "Terminated",
        16 => "Stack fault",
        17 => "Child exited",
        18 => "Continued",
        19 => "Stopped (signal)",
        20 => "Stopped",
        21 => "Stopped (tty input)",
        22 => "Stopped (tty output)",
        23 => "Urgent I/O condition",
        24 => "CPU time limit exceeded",
        25 => "File size limit exceeded",
        26 => "Virtual timer expired",
        27 => "Profiling timer expired",
        28 => "Window changed",
        29 => "I/O possible",
        30 => "Power failure",
        31 => "System call disallowed",
        0 | _ => "Unknown signal",
    };
    s.as_ptr()
}
