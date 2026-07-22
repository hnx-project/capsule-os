//! # 🧭 Path Normalization Utilities
//!
//! `path` provides low-level stack-allocated utilities for parsing and
//! normalising file system paths without any dynamic heap allocation.

use crate::syscalls;

/// Helper to normalise a given raw file-system path against CWD in a stack-only,
/// zero-dynamic-allocation manner.
pub fn normalise_path(path: *const u8, out: &mut [u8]) -> Result<usize, ()> {
    if path.is_null() || out.is_empty() {
        return Err(());
    }

    let mut raw_len = 0;
    unsafe {
        while *path.add(raw_len) != 0 && raw_len < 127 {
            raw_len += 1;
        }
    }
    let raw_bytes = unsafe { core::slice::from_raw_parts(path, raw_len) };

    let is_absolute = raw_len > 0 && raw_bytes[0] == b'/';

    let mut temp = [0u8; 512];
    let mut temp_len = 0;

    if !is_absolute {
        let mut cwd_buf = [0u8; 256];
        match syscalls::getcwd(&mut cwd_buf) {
            Ok(len) if len > 0 => {
                let actual_cwd = &cwd_buf[..len];
                let to_copy = actual_cwd.len().min(temp.len());
                temp[..to_copy].copy_from_slice(&actual_cwd[..to_copy]);
                temp_len = to_copy;
            }
            _ => {
                temp[0] = b'/';
                temp_len = 1;
            }
        }
        
        if temp_len > 0 && temp[temp_len - 1] != b'/' {
            if temp_len < temp.len() {
                temp[temp_len] = b'/';
                temp_len += 1;
            }
        }

        let space_remaining = temp.len() - temp_len;
        let to_copy = raw_bytes.len().min(space_remaining);
        temp[temp_len..temp_len + to_copy].copy_from_slice(&raw_bytes[..to_copy]);
        temp_len += to_copy;
    } else {
        let to_copy = raw_bytes.len().min(temp.len());
        temp[..to_copy].copy_from_slice(&raw_bytes[..to_copy]);
        temp_len = to_copy;
    }

    let temp_slice = &temp[..temp_len];
    
    let mut comp_starts = [0usize; 32];
    let mut comp_lens = [0usize; 32];
    let mut comp_count = 0;

    let mut i = 0;
    while i < temp_len {
        while i < temp_len && temp_slice[i] == b'/' {
            i += 1;
        }
        if i >= temp_len {
            break;
        }
        let start = i;
        while i < temp_len && temp_slice[i] != b'/' {
            i += 1;
        }
        let len = i - start;
        let comp = &temp_slice[start..start + len];

        if comp == b"." {
            // ignore
        } else if comp == b".." {
            if comp_count > 0 {
                comp_count -= 1;
            }
        } else {
            if comp_count < 32 {
                comp_starts[comp_count] = start;
                comp_lens[comp_count] = len;
                comp_count += 1;
            } else {
                return Err(());
            }
        }
    }

    let mut out_idx = 0;
    out[out_idx] = b'/';
    out_idx += 1;

    for c in 0..comp_count {
        if c > 0 {
            if out_idx >= out.len() { return Err(()); }
            out[out_idx] = b'/';
            out_idx += 1;
        }
        let start = comp_starts[c];
        let len = comp_lens[c];
        if out_idx + len >= out.len() {
            return Err(());
        }
        out[out_idx..out_idx + len].copy_from_slice(&temp_slice[start..start + len]);
        out_idx += len;
    }

    if out_idx >= out.len() {
        return Err(());
    }
    out[out_idx] = 0;

    Ok(out_idx)
}
