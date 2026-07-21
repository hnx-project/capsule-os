//! # 📦 Program Sandbox Loader Subsystem
//!
//! This module implements the `ProgramLoader` struct, which is designed for spawning
//! standard sandboxed applications from the read-only boot-time archive (`BootFS`).
//!
//! Unlike `ServiceLoader` which initializes privileged background services, `ProgramLoader`
//! handles ordinary temporary/interactive CLI utilities (e.g., `testall`) with standard priority.

use crate::service::ServiceLoader;
use shared::status::Result;

/// Loader specifically for ordinary applications/programs (runs from BootFS VMO).
///
/// Under CapsuleOS's microkernel capability design, spawning a program is a unified,
/// clean, and atomic operation that does not use the traditional UNIX `fork()`.
/// Instead, `ProgramLoader` directly parses the `HNXF_VFS` partition, creates a cloned VMO,
/// and asks the kernel to initialize a brand-new container for the program.
///
/// For running processes that need to replace themselves with a new binary, use the POSIX `execv()`
/// library call instead, which performs process image replacement on the active VMAR rather than
/// creating a new container.
pub struct ProgramLoader {
    loader: ServiceLoader,
}

impl ProgramLoader {
    /// Create a new ProgramLoader bound to the raw BootFS VMO handle.
    ///
    /// # Parameters
    /// - `bootfs_vmo`: The raw `HandleValue` index referencing the boot memory filesystem.
    pub const fn new(bootfs_vmo: usize) -> Self {
        Self {
            loader: ServiceLoader::new(bootfs_vmo),
        }
    }

    /// Spawn a program into execution inside its own sandboxed process container.
    ///
    /// This retrieves the offset and size of the executable, clones a read-only child VMO,
    /// and invokes the atomic binary loading syscall.
    ///
    /// # Parameters
    /// - `name`: Relative or absolute file path/name of the target binary within BootFS.
    ///
    /// # Errors
    /// - `Status::NotFound`: If the executable name is not present in BootFS.
    /// - `Status::WrongType`: If the Superblock format is invalid.
    /// - `Status::InvalidImage`: If directory parsing fails or if the image size exceeds limits.
    /// - `Status::NoMemory`: If the kernel runs out of physical/virtual memory during process creation.
    pub fn spawn_program(&self, name: &str) -> Result<usize> {
        // 1. Resolve offset and size of the program within BootFS
        let (offset, size) = self.loader.resolve_file(name)?;

        // 2. Clone a read-only child VMO segment representing the program's binary
        let aligned_offset = offset & !(4096 - 1);
        let aligned_size = ((offset - aligned_offset) + size + 4095) & !(4095);
        let program_vmo =
            crate::syscalls::vmo_create_child(self.loader.bootfs_vmo(), aligned_offset, aligned_size)?;

        // 3. Atomically load and start the program via the kernel's load_binary interface
        let intra_page_offset = offset - aligned_offset;
        let handle = crate::syscalls::load_binary(program_vmo, name, intra_page_offset)?;

        // Clean up temporary child VMO handle
        let _ = crate::syscalls::close(program_vmo);

        Ok(handle as usize)
    }
}
