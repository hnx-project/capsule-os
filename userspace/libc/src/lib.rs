#![no_std]

pub mod syscalls;

pub use syscalls::*;

pub fn putchar(_c: u8) {
}

pub fn getchar() -> Option<u8> {
    None
}

extern "C" {
    pub fn _start();
}
