//! POSIX `getenv / setenv / unsetenv / environ` for `libc`.
//!
//! S1 hard-codes a single process's environment into a fixed-size
//! static array.  Bash needs `getenv("PATH")`, `getenv("HOME")`,
//! `setenv("VAR", "val")` etc; we seed the array during
//! `env_init()` (called from `_hnx_user_entry`) and let the
//! program mutate it from there.
//!
//! Storage layout:
//!   `ENV_TABLE[i].bytes` is a `KEY=VALUE\0` blob up to 96 bytes.
//!   `ENV_TABLE[i].used` flags a live slot.

pub const ENV_SLOTS: usize = 64;
pub const ENV_KV_BYTES: usize = 96;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EnvSlot {
    pub bytes: [u8; ENV_KV_BYTES],
    pub used: bool,
}

const EMPTY_SLOT: EnvSlot = EnvSlot {
    bytes: [0; ENV_KV_BYTES],
    used: false,
};

#[no_mangle]
pub static mut ENV_TABLE: [EnvSlot; ENV_SLOTS] = [EMPTY_SLOT; ENV_SLOTS];

/// Pointer array of `*const u8` matching `extern char **environ`
/// convention.  Updated by `env_rebuild_pointers`.
#[no_mangle]
pub static mut ENV_PTRS: [*const u8; ENV_SLOTS] = [core::ptr::null(); ENV_SLOTS];
#[no_mangle]
pub static mut ENV_COUNT: usize = 0;

#[no_mangle]
pub static mut environ: *mut *const u8 = core::ptr::null_mut();

/// Helper used by libc::setenv to extract the value into a
/// fixed-size scratch buffer.  Returns `(len, bytes)` where
/// `bytes` is the inner scratch (lives only for this call).
fn read_cstr_into(src: *const u8, scratch: &mut [u8]) -> usize {
    if src.is_null() {
        return 0;
    }
    let mut i = 0;
    while i < scratch.len() {
        let b = unsafe { *src.add(i) };
        if b == 0 {
            return i;
        }
        scratch[i] = b;
        i += 1;
    }
    i
}

fn env_rebuild_pointers() {
    unsafe {
        let mut count = 0usize;
        for i in 0..ENV_SLOTS {
            if ENV_TABLE[i].used {
                ENV_PTRS[count] = ENV_TABLE[i].bytes.as_ptr();
                count += 1;
            }
        }
        ENV_COUNT = count;
        environ = ENV_PTRS.as_mut_ptr();
    }
}

fn slot_key_eq<'a>(slot: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    if slot.is_empty() {
        return None;
    }
    let mut eq_at = 0;
    while eq_at < slot.len() && slot[eq_at] != 0 && slot[eq_at] != b'=' {
        eq_at += 1;
    }
    if eq_at != key.len() {
        return None;
    }
    if &slot[..eq_at] != key {
        return None;
    }
    let mut value_end = eq_at;
    if value_end < slot.len() && slot[value_end] == b'=' {
        value_end += 1;
    }
    while value_end < slot.len() && slot[value_end] != 0 {
        value_end += 1;
    }
    Some(&slot[eq_at + 1..value_end])
}

/// Set an environment variable from byte slices.  Used by
/// `env_init()`.
pub fn setenv_bytes(key: &[u8], value: &[u8]) -> i32 {
    if key.is_empty() || key.contains(&b'=') || key.len() + 1 + value.len() + 1 > ENV_KV_BYTES {
        return -1;
    }
    unsafe {
        for i in 0..ENV_SLOTS {
            if !ENV_TABLE[i].used {
                continue;
            }
            let raw = core::slice::from_raw_parts(
                ENV_TABLE[i].bytes.as_ptr(),
                ENV_TABLE[i].bytes.len(),
            );
            if slot_key_eq(raw, key).is_some() {
                // Overwrite in-place.  The slot's bytes already
                // contain a `KEY=` prefix; reuse those bytes and
                // write the value after the `=`.
                let slot = &mut ENV_TABLE[i].bytes;
                let eq_at = key.len();
                slot[eq_at] = b'=';
                let mut p = eq_at + 1;
                for &b in value {
                    slot[p] = b;
                    p += 1;
                }
                slot[p] = 0;
                // Zero any trailing bytes from a longer previous
                // value so a stale tail doesn't appear when the
                // caller reads back via `getenv`.
                while p < slot.len() - 1 {
                    if slot[p] != 0 {
                        slot[p] = 0;
                    } else {
                        // Already a single terminating NUL.
                        break;
                    }
                    p += 1;
                }
                env_rebuild_pointers();
                return 0;
            }
        }
        for i in 0..ENV_SLOTS {
            if !ENV_TABLE[i].used {
                let slot = &mut ENV_TABLE[i].bytes;
                let mut p = 0;
                for &b in key {
                    slot[p] = b;
                    p += 1;
                }
                slot[p] = b'=';
                p += 1;
                for &b in value {
                    slot[p] = b;
                    p += 1;
                }
                slot[p] = 0;
                ENV_TABLE[i].used = true;
                env_rebuild_pointers();
                return 0;
            }
        }
    }
    -1
}

#[no_mangle]
pub unsafe extern "C" fn setenv(key: *const u8, value: *const u8) -> i32 {
    if key.is_null() {
        return -1;
    }
    let mut kbuf = [0u8; 64];
    let klen = read_cstr_into(key, &mut kbuf);
    if klen == 0 || klen > ENV_KV_BYTES - 1 {
        return -1;
    }
    let mut vbuf = [0u8; 96];
    let vlen = if value.is_null() { 0 } else { read_cstr_into(value, &mut vbuf) };
    setenv_bytes(&kbuf[..klen], &vbuf[..vlen])
}

#[no_mangle]
pub unsafe extern "C" fn unsetenv(key: *const u8) -> i32 {
    if key.is_null() {
        return -1;
    }
    let mut kbuf = [0u8; 64];
    let klen = read_cstr_into(key, &mut kbuf);
    if klen == 0 {
        return -1;
    }
    let mut found = false;
    for i in 0..ENV_SLOTS {
        if !ENV_TABLE[i].used {
            continue;
        }
        let raw = core::slice::from_raw_parts(
            ENV_TABLE[i].bytes.as_ptr(),
            ENV_TABLE[i].bytes.len(),
        );
        if slot_key_eq(raw, &kbuf[..klen]).is_some() {
            ENV_TABLE[i].used = false;
            ENV_TABLE[i].bytes = [0; ENV_KV_BYTES];
            found = true;
            break;
        }
    }
    if found {
        env_rebuild_pointers();
        0
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn getenv(key: *const u8) -> *const u8 {
    if key.is_null() {
        return core::ptr::null();
    }
    let mut kbuf = [0u8; 64];
    let klen = read_cstr_into(key, &mut kbuf);
    if klen == 0 {
        return core::ptr::null();
    }
    for i in 0..ENV_SLOTS {
        if !ENV_TABLE[i].used {
            continue;
        }
        let raw = core::slice::from_raw_parts(
            ENV_TABLE[i].bytes.as_ptr(),
            ENV_TABLE[i].bytes.len(),
        );
        if let Some(value_slice) = slot_key_eq(raw, &kbuf[..klen]) {
            return value_slice.as_ptr();
        }
    }
    core::ptr::null()
}

/// C `environ` accessor for callers who want to iterate env
/// without going through `libc::*` wrappers.
#[no_mangle]
pub extern "C" fn __hnx_environ() -> *const *const u8 {
    unsafe { environ }
}

/// Seed the environment with the minimum bash / osh need at
/// startup: `USER`, `HOME`, `PWD`, `PATH`, `SHELL`, `TERM`, `IFS`.
pub fn env_init() {
    let _ = unsafe { setenv(b"USER\0".as_ptr(), b"root\0".as_ptr()) };
    let _ = unsafe { setenv(b"HOME\0".as_ptr(), b"/root\0".as_ptr()) };
    let _ = unsafe { setenv(b"PWD\0".as_ptr(), b"/\0".as_ptr()) };
    let _ = unsafe { setenv(b"SHELL\0".as_ptr(), b"/system/bin/osh\0".as_ptr()) };
    let _ = unsafe {
        setenv(
            b"PATH\0".as_ptr(),
            b"/system/bin:/system/sbin:/bin:/sbin\0".as_ptr(),
        )
    };
    let _ = unsafe { setenv(b"TERM\0".as_ptr(), b"dumb\0".as_ptr()) };
    let _ = unsafe { setenv(b"IFS\0".as_ptr(), b" \t\n\0".as_ptr()) };
}
