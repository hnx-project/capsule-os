#![no_std]
#![forbid(unsafe_code)]

pub mod status;
pub mod types;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

pub use status::{Result, Status};
pub use types::{HandleValue, ObjectType};
