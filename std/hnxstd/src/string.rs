//! B9 (`KERNEL_HEALTH.md` B9): minimal #![no_std]-capable
//! `String` using a backing `Vec<u8>`.  UTF-8 validation is
//! NOT done -- callers are expected to feed ASCII bytes, the
//! common case for 1.0 (errno names, log lines, env-var
//! assignments).
//!
//! In 1.0 the static capacity is 256 bytes (matching the Vec
//! backing); users that need more can call `extend_from_u8`.

use crate::vec::Vec;

pub struct String {
    buf: Vec<u8>,
}

impl String {
    pub const fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn from_str(s: &str) -> Self {
        let mut s2 = String::new();
        for &b in s.as_bytes() {
            let _ = s2.buf.push(b);
        }
        s2
    }

    pub fn from_byte(b: u8) -> Self {
        let mut s = String::new();
        let _ = s.buf.push(b);
        s
    }

    pub fn as_str(&self) -> &str {
        unsafe {
            core::str::from_utf8_unchecked(self.buf.as_slice())
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.buf.as_slice()
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn push_byte(&mut self, b: u8) -> Result<(), &'static str> {
        self.buf.push(b)
    }

    pub fn push_str(&mut self, s: &str) -> Result<(), &'static str> {
        for &b in s.as_bytes() {
            self.buf.push(b)?;
        }
        Ok(())
    }
}
