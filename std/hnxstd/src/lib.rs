//! hnxstd: a from-scratch #![no_std] standard library that
//! lives next to the sysroot descriptors under std/.  1.0 only
//! fills in the most-used Rust surfaces so that EL0 user-space
//! programs can avoid rolling a hand-coded `dec_digits` etc.
//! every time they need to print a number:
//!
//!   - `vec::Vec<T>`           - heap-backed dynamic array
//!   - `string::String`        - owned byte strings
//!   - `fmt::format!`          - minimum format macro
//!   - `io::println/print`     - UART-bound stdio
//!
//! Everything else (allocators, HashMap, threads, BTreeMap) is
//! intentionally out of 1.0 scope and lives in 1.1+.  See
//! KERNEL_HEALTH.md B9 for the closed-source 0.5 era and the
//! 1.0 minimum.

#![no_std]

pub mod io;
pub mod string;
pub mod thread;
pub mod vec;
pub mod fmt;
