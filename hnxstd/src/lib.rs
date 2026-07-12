#![no_std]

pub mod io;
pub mod thread;

pub mod string {
    pub struct String {
        _data: &'static str,
    }

    impl String {
        pub fn new() -> Self {
            String { _data: "" }
        }
        pub fn from_str(s: &'static str) -> Self {
            String { _data: s }
        }
        pub fn as_str(&self) -> &str {
            self._data
        }
        pub fn as_bytes(&self) -> &[u8] {
            self._data.as_bytes()
        }
    }
}
