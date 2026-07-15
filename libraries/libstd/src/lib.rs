#![no_std]

pub mod env;
pub mod fmt;
pub mod fs;
pub mod io;
pub mod os;
pub mod process;
pub mod string;
pub mod thread;
pub mod vec;

// Re-export common macros so standard userland matches Rust std perfectly
pub use core::format_args;

// Provide standard prelude
pub mod prelude {
    pub mod v1 {
        pub use crate::io::{print, println};
        pub use crate::string::String;
        pub use crate::vec::Vec;
        pub use core::prelude::v1::*;
    }
}
