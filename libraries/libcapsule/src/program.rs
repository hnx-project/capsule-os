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

    /// S7 / procmgr std-fd handoff.  Spawns `name` as a brand-new
    /// process that shares the spawner's `(stdin, stdout,
    /// stderr)` channel handles.  The supplied three channel
    /// handles are placed in the new process's `fd_table[0..=2]`;
    /// a `0` entry leaves the corresponding slot at the
    /// kernel-builtin UART path.
    ///
    /// The wrapper is intentionally kept thin — everything else
    /// (the binary VMO walk, argv VMO construction, etc.) is
    /// identical to the non-std path.  We avoid the
    /// `load_binary` exec-replaces-caller path on purpose so
    /// procmgr can keep running as a background service while
    /// its children spin up.
    pub fn spawn_program_with_std_fds(
        &self,
        name: &str,
        stdin: u32,
        stdout: u32,
        stderr: u32,
    ) -> Result<usize> {
        // 1. Resolve offset and size of the program within BootFS
        let (offset, size) = self.loader.resolve_file(name)?;

        // 2. Clone a read-only child VMO segment representing
        //    the program's binary.
        let aligned_offset = offset & !(4096 - 1);
        let aligned_size = ((offset - aligned_offset) + size + 4095) & !(4095);
        let program_vmo = crate::syscalls::vmo_create_child(
            self.loader.bootfs_vmo(),
            aligned_offset,
            aligned_size,
        )?;

        // 3. Build the std-fds vmo: 12 bytes packed as three
        //    little-endian u32 channel handles.  A 0 entry means
        //    "kernel-builtin UART for this slot".
        let std_fds_vmo = crate::syscalls::vmo_create(12)?;
        let std_packed: [u8; 12] = [
            (stdin  & 0xff) as u8, ((stdin  >> 8) & 0xff) as u8,
            ((stdin  >> 16) & 0xff) as u8, ((stdin  >> 24) & 0xff) as u8,
            (stdout & 0xff) as u8, ((stdout >> 8) & 0xff) as u8,
            ((stdout >> 16) & 0xff) as u8, ((stdout >> 24) & 0xff) as u8,
            (stderr & 0xff) as u8, ((stderr >> 8) & 0xff) as u8,
            ((stderr >> 16) & 0xff) as u8, ((stderr >> 24) & 0xff) as u8,
        ];
        crate::syscalls::vmo_write(std_fds_vmo, 0, &std_packed)?;

        // 4. Build the argv vmo.  For an `argv = [name]` of
        //    1 argument the encoding is: `[u32 count=1]
        //    [u32 strlen(name)] [bytes]`.
        let argv = name.as_bytes();
        let argv_len = argv.len();
        let argv_vmo = crate::syscalls::vmo_create(4 + 4 + argv_len)?;
        let mut argv_buf = [0u8; 4 + 4 + 64];
        if argv_len > 64 {
            let _ = crate::syscalls::close(argv_vmo);
            return Err(shared::status::Status::InvalidArgs);
        }
        argv_buf[0..4].copy_from_slice(&1u32.to_le_bytes());
        argv_buf[4..8].copy_from_slice(&(argv_len as u32).to_le_bytes());
        argv_buf[8..8 + argv_len].copy_from_slice(argv);
        crate::syscalls::vmo_write(argv_vmo, 0, &argv_buf[..8 + argv_len])?;

        // 5. Hand control to the kernel.
        let pid = crate::syscall!(
            shared::syscall_nums::SYSCALL_SPAWN_STD,
            program_vmo as usize,
            argv_vmo as usize,
            std_fds_vmo as usize,
            0,
            0,
            0
        );

        // 6. Close our local handles — the kernel has its own
        //    references to the underlying VMO objects.
        let _ = crate::syscalls::close(program_vmo);
        let _ = crate::syscalls::close(argv_vmo);
        let _ = crate::syscalls::close(std_fds_vmo);

        if (pid as isize) < 0 {
            Err(shared::status::Status::from_raw(pid as i32))
        } else {
            Ok(pid as usize)
        }
    }
}
