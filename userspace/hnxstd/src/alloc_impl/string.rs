use super::vec::Vec;

pub struct String {
    buf: Vec<u8>,
}

impl String {
    pub fn new() -> Self { String { buf: Vec::new() } }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf).unwrap_or("")
    }
    pub fn as_bytes(&self) -> &[u8] { &self.buf }
}
