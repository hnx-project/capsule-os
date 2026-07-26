//! `strings_extra` — BSD/POSIX extensions that bash/readline's
//! autoconf probes but that aren't in the canonical `<string.h>`.
//!
//! Coverage targets (from bash 5.3 / readline 8.x probes):
//!   bcopy, bzero.
//!
//! The `asprintf`/`snprintf`/`printf` family is intentionally
//! *not* implemented in this build.  Stable Rust's `...` in
//! `extern "C"` requires the unstable `c_variadic` feature
//! which we do not pull in to keep the libc crate on stable.
//! bash's `lib/intl/asprintf.c` autoconf probe detects
//! `vasprintf` via the autoconf `AC_CHECK_FUNCS` macro which
//! compiles a tiny harness program; that harness will fail
//! to link at the bash-side for now.  Once bash falls back to
//! its bundled `lib/intl/vasprintf.c` (which it always
//! builds locally when the system lacks `vasprintf`), bash
//! proceeds normally — bash ships its own copy.

/// POSIX `bcopy(src, dst, n)` — copy `n` bytes from `src` to
/// `dst`; arguments are reversed vs `memcpy` to match BSD /
/// legacy libc.  We treat overlapping copies as `memmove`.
#[no_mangle]
pub extern "C" fn bcopy(src: *const u8, dst: *mut u8, n: usize) {
    if src.is_null() || dst.is_null() || n == 0 {
        return;
    }
    let sp = src as usize;
    let dp = dst as usize;
    if dp < sp || dp >= sp + n {
        // Forward, non-overlapping.
        unsafe {
            core::ptr::copy_nonoverlapping(src, dst, n);
        }
    } else {
        // Backward (overlap-safe).
        for i in (0..n).rev() {
            unsafe {
                *dst.add(i) = *src.add(i);
            }
        }
    }
}

/// POSIX `bzero(s, n)` — set the first `n` bytes of `s` to zero.
#[no_mangle]
pub extern "C" fn bzero(s: *mut u8, n: usize) {
    if s.is_null() || n == 0 {
        return;
    }
    unsafe {
        core::ptr::write_bytes(s, 0u8, n);
    }
}
