//! libstd: a from-scratch #![no_std] standard library.
//!
//!   - `vec::Vec<T>`           - heap-backed dynamic array
//!   - `string::String`        - owned byte strings
//!   - `fmt::format!`          - minimum format macro
//!   - `io::println/print`     - UART-bound stdio

#![no_std]

pub mod fmt;
pub mod io;
pub mod string;
pub mod thread;
pub mod vec;

pub use io::process::Process;
pub use io::vmo::Vmo;
