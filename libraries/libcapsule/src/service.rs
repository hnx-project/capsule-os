//! # 📦 Service Loader Subsystem
//!
//! This module implements the `ServiceLoader` struct, which is designed for spawning
//! privileged background services (such as `fileagent` and `devmgr`) from the read-only BootFS.
//!
//! Under CapsuleOS, services run indefinitely, have higher execution priorities, and have access
//! to core system capabilities (e.g. MMIO mapping, port queues, global service registration).

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
///
/// This serves as the primary system-level bootstrap interface. In future revisions,
/// the low-level parsing and child VMO cloning logic will be unified under a cohesive
/// `BootfsLoader` struct, while preserving `ServiceLoader` as a distinct, intent-based API.
pub struct ServiceLoader {
    bootfs_vmo: usize,
}

impl ServiceLoader {
    /// Create a new ServiceLoader bound to the raw BootFS VMO handle.
    ///
    /// # Parameters
    /// - `bootfs_vmo`: The raw `HandleValue` index referencing the boot memory filesystem.
    pub const fn new(bootfs_vmo: usize) -> Self {
        Self { bootfs_vmo }
    }

    /// Get the raw BootFS VMO handle.
    pub fn bootfs_vmo(&self) -> usize {
        self.bootfs_vmo
    }

    /// Spawn a service by resolving its filename from the BootFS archive header,
    /// cloning its executable chunk into a VMO, and starting its EL0 process.
    ///
    /// # Parameters
    /// - `name`: Relative or absolute file path/name of the target service binary.
    ///
    /// # Errors
    /// - `Status::NotFound`: If the filename is not present in BootFS.
    /// - `Status::WrongType`: If the Superblock signature does not match "HNXF_VFS".
    /// - `Status::InvalidImage`: If directory metadata is corrupted.
    pub fn spawn_service(&self, name: &str) -> Result<usize> {
        let (offset, size) = self.resolve_file(name)?;

        // 1. Clone a read-only child VMO segment from the BootFS VMO representing the binary.
        let aligned_offset = offset & !(4096 - 1);
        let aligned_size = ((offset - aligned_offset) + size + 4095) & !(4095);
        let service_vmo =
            crate::syscalls::vmo_create_child(self.bootfs_vmo, aligned_offset, aligned_size)?;

        // 2. Load and start the binary. Inside the kernel, sys_load_binary automatically creates and runs the process.
        let intra_page_offset = offset - aligned_offset;
        let pid_or_handle = crate::syscalls::load_binary(service_vmo, name, intra_page_offset)?;

        // Clean up temporary child VMO handle to avoid capability leak in the loader's table.
        let _ = crate::syscalls::close(service_vmo);

        Ok(pid_or_handle as usize)
    }

    /// Parse the HNXF_VFS superblock and directory headers to find the offset and size of a file.
    ///
    /// This performs synchronous reading on the `bootfs_vmo` via capability system calls.
    pub(crate) fn resolve_file(&self, filename: &str) -> Result<(usize, usize)> {
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

/// Unified Public Interface for locating and connecting to system services.
pub struct ServiceManager;

impl ServiceManager {
    /// Connect to a system service by name and obtain its communication channel handle.
    pub fn connect(name: &str) -> Result<usize> {
        crate::syscalls::channel_lookup(name)
    }
}

/// Notify the Init Service (initd) that this service has finished initialization
/// and is ready to accept client connections.
pub fn notify_init(service_name: &str) -> Result<()> {
    if let Ok(init_chan) = crate::syscalls::channel_lookup("svc.init") {
        let mut msg = [0u8; 148];
        msg[0] = 1; // INIT_CMD_READY
        let name_bytes = service_name.as_bytes();
        let len = name_bytes.len().min(128);
        msg[4..8].copy_from_slice(&(len as u32).to_le_bytes());
        msg[20..20 + len].copy_from_slice(&name_bytes[..len]);
        let _ = crate::syscalls::channel_write(init_chan, &msg, &[]);
        let _ = crate::syscalls::close(init_chan);
    }
    Ok(())
}
