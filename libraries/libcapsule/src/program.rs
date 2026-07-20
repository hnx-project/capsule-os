use crate::service::ServiceLoader;
use shared::status::Result;

/// Loader specifically for ordinary applications/programs (runs from BootFS VMO).
pub struct ProgramLoader {
    loader: ServiceLoader,
}

impl ProgramLoader {
    /// Create a new ProgramLoader bound to the raw BootFS VMO handle.
    pub const fn new(bootfs_vmo: usize) -> Self {
        Self {
            loader: ServiceLoader::new(bootfs_vmo),
        }
    }

    /// Spawn a program into execution inside its own sandboxed process container.
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
