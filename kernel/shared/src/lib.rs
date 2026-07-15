#![no_std]
#![forbid(unsafe_code)]

pub mod status;
pub mod syscall_nums;
pub mod types;
pub mod launcher;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

pub use status::{Result, Status};
pub use syscall_nums::*;
pub use types::{HandleValue, ObjectType};
pub use launcher::{ServiceDescriptor, BootFsHeader, BootFsEntry, AlignmentHelper, MAGIC_BOOTFS};
