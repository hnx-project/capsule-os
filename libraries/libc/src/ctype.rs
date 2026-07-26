//! `ctype` — POSIX `<ctype.h>` ASCII classification.
//!
//! CapsuleOS's libc is a small `#![no_std]` build that links
//! each program statically; pulling in the upstream Rust
//! `char` Unicode tables would bloat every EL0 binary by ~10
//! KiB for the ASCII-only classification bash/readline care
//! about.  The eight functions here are byte-only, ASCII-only,
//! and 'sufficient' for any program that doesn't actually
//! care about UTF-8 graphemes (most POSIX shell utilities).
//!
//! Coverage targets (from bash 5.3 autoconf probes):
//!   isascii, isblank, isgraph, isprint, isspace, isxdigit,
//!   tolower, toupper.

#![allow(unused_imports)]

const CT_UP: u8 = 0x01; // upper-case A-Z
const CT_LOW: u8 = 0x02; // lower-case a-z
const CT_DIG: u8 = 0x04; // digit 0-9
const CT_HEX: u8 = 0x08; // hex digit (includes a-f / A-F)
const CT_SPC: u8 = 0x10; // space, \f, \n, \r, \t, \v
const CT_PRT: u8 = 0x20; // printable (graph + space)
const CT_BLK: u8 = 0x40; // blank (space, \t)
const CT_ASC: u8 = 0x80; // 7-bit ASCII (0..=0x7F)

/// Compact classification table for all 256 byte values.
///
/// Indexed by the byte itself; the resulting flags are the
/// union of every category the byte belongs to.  This costs 256
/// bytes of `.rodata` per program (acceptable in EL0) and lets
/// each predicate compile to a single `flags[byte] & CAT` test.
static CT_FLAGS: [u8; 256] = {
    let mut t = [0u8; 256];
    // ASCII 7-bit bit.
    let mut i: usize = 0;
    while i <= 0x7F {
        t[i] |= CT_ASC;
        i += 1;
    }
    // Digits + hex.
    let mut i: usize = 0;
    while i < 10 {
        t[b'0' as usize + i] |= CT_DIG | CT_HEX;
        i += 1;
    }
    // Upper-case.
    let mut i: usize = 0;
    while i < 26 {
        let b = b'A' as usize + i;
        t[b] |= CT_UP | CT_HEX;
        t[b + 0x20] |= CT_LOW; // mirror lowercase via 'a'
        i += 1;
    }
    // Lower-case.
    let mut i: usize = 0;
    while i < 26 {
        t[b'a' as usize + i] |= CT_LOW | CT_HEX;
        i += 1;
    }
    // Spaces.
    t[b' ' as usize]  |= CT_SPC | CT_PRT | CT_BLK;
    t[b'\t' as usize] |= CT_SPC | CT_BLK;
    t[b'\n' as usize] |= CT_SPC;
    t[b'\r' as usize] |= CT_SPC;
    t[b'\x0B' as usize] |= CT_SPC; // vertical tab
    t[b'\x0C' as usize] |= CT_SPC; // form feed
    // Printable but not space.
    let mut i: usize = 0x21;
    while i <= 0x7E {
        t[i] |= CT_PRT;
        i += 1;
    }
    t
};

#[inline(always)]
fn flags(b: u8) -> u8 {
    CT_FLAGS[b as usize]
}

#[no_mangle]
pub extern "C" fn isascii(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        1
    }
}

#[no_mangle]
pub extern "C" fn isblank(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_BLK != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isgraph(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        let f = flags(c as u8);
        ((f & CT_PRT != 0) && (f & CT_SPC == 0)) as i32
    }
}

#[no_mangle]
pub extern "C" fn isprint(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_PRT != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isspace(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_SPC != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isxdigit(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_HEX != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isdigit(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_DIG != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isalpha(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        let f = flags(c as u8);
        ((f & (CT_UP | CT_LOW)) != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isalnum(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        let f = flags(c as u8);
        ((f & (CT_UP | CT_LOW | CT_DIG)) != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn isupper(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_UP != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn islower(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        0
    } else {
        (flags(c as u8) & CT_LOW != 0) as i32
    }
}

#[no_mangle]
pub extern "C" fn tolower(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        c
    } else if flags(c as u8) & CT_UP != 0 {
        c + (b'a' as i32 - b'A' as i32)
    } else {
        c
    }
}

#[no_mangle]
pub extern "C" fn toupper(c: i32) -> i32 {
    if c < 0 || c > 0x7F {
        c
    } else if flags(c as u8) & CT_LOW != 0 {
        c - (b'a' as i32 - b'A' as i32)
    } else {
        c
    }
}
