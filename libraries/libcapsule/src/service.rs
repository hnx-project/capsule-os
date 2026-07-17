use shared::status::{Result, Status};

/// An entry descriptor inside the HNXF_VFS archive (144 bytes).
#[repr(C)]
struct HnxfEntry {
    path: [u8; 128],
    offset: u64,
    size: u64,
}

/// User-space BootFS loader. Resolves and spawns system service binaries
/// from the read-only memory BootFS VMO without using any file system paths.
pub struct ServiceLoader {
    bootfs_vmo: usize,
}

impl ServiceLoader {
    /// Create a new ServiceLoader bound to the raw BootFS VMO handle.
    pub const fn new(bootfs_vmo: usize) -> Self {
        Self { bootfs_vmo }
    }

    /// Spawn a service by resolving its filename from the BootFS archive header,
    /// cloning its executable chunk into a VMO, and starting its EL0 process.
    pub fn spawn_service(&self, name: &str) -> Result<usize> {
        let (offset, size) = self.resolve_file(name)?;

        // 1. Create a blank process container.
        let proc_handle = crate::syscalls::process_create(name)?;

        // 2. Clone a read-only child VMO segment from the BootFS VMO representing the binary.
        let service_vmo = crate::syscalls::vmo_create_child(self.bootfs_vmo, offset, size)?;

        // 3. Load and map the ELF/OHLINK binary directly into the new process's VMAR.
        // The return value of load_binary is the relocated user-mode entry PC!
        let entry_pc = crate::syscalls::load_binary(service_vmo, name)?;

        // 4. Create its primary execution thread.
        let thread_handle = crate::syscalls::thread_create(
            proc_handle as u32,
            entry_pc as usize, // Use the relocated dynamic entry PC!
            0,                 // arg0 (unused for initial boot thread)
            0,                 // arg1 (unused)
            128 * 1024,        // 128 KiB stack size
        )?;

        // 5. Fire off the thread.
        crate::syscalls::thread_start(thread_handle as u32)?;

        // Clean up temporary child VMO handle to avoid capability leak in the loader's table.
        let _ = crate::syscalls::close(service_vmo);
        let _ = crate::syscalls::close(thread_handle);

        Ok(proc_handle)
    }

    /// Parse the HNXF_VFS superblock and directory headers to find offset/size of a file.
    fn resolve_file(&self, filename: &str) -> Result<(usize, usize)> {
        // 1. Read Superblock: Magic (8 bytes) + Count (8 bytes) = 16 bytes.
        let mut superblock = [0u8; 16];
        crate::syscalls::vmo_read(self.bootfs_vmo, 0, &mut superblock)?;

        if &superblock[0..8] != b"HNXF_VFS" {
            return Err(Status::WrongType);
        }

        let count = u64::from_le_bytes(superblock[8..16].try_into().unwrap()) as usize;
        if count == 0 || count > 128 {
            return Err(Status::InvalidImage);
        }

        // 2. Scan directory entries (each entry is 144 bytes).
        let mut entry_bytes = [0u8; 144];
        for i in 0..count {
            let offset = 16 + (i * 144);
            crate::syscalls::vmo_read(self.bootfs_vmo, offset, &mut entry_bytes)?;

            let entry_path_len = entry_bytes[0..128]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(128);
            let entry_path = &entry_bytes[0..entry_path_len];

            // Match relative path. Note that we check if the entry's filename contains or matches
            // the name we are searching for (e.g., matching "system/bin/devmgr" or just "devmgr").
            let entry_str = core::str::from_utf8(entry_path).map_err(|_| Status::InvalidImage)?;
            if entry_str == filename || entry_str.ends_with(filename) {
                let file_offset =
                    u64::from_le_bytes(entry_bytes[128..136].try_into().unwrap()) as usize;
                let file_size =
                    u64::from_le_bytes(entry_bytes[136..144].try_into().unwrap()) as usize;
                return Ok((file_offset, file_size));
            }
        }

        Err(Status::NotFound)
    }
}
