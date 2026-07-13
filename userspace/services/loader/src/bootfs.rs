use hnxstd::{Process, Vmo};
use shared::status::Status;

const MAGIC_BOOTFS: &[u8; 8] = b"HNXF_VFS";

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct BootFsHeader {
    magic: [u8; 8],
    entry_count: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct BootFsEntry {
    pub path: [u8; 128],
    pub offset: u64,
    pub size: u64,
}

/// Iterator pattern for scanning through BootFS images lazily
pub struct BootFsScanner<'a> {
    vmo: &'a Vmo,
    current_entry: usize,
    total_entries: usize,
    current_offset: usize,
}

impl<'a> BootFsScanner<'a> {
    pub fn new(vmo: &'a Vmo) -> Result<Self, Status> {
        let header: BootFsHeader = vmo.read_struct(0)?;
        if header.magic != *MAGIC_BOOTFS {
            return Err(Status::InvalidArgs);
        }
        Ok(Self {
            vmo,
            current_entry: 0,
            total_entries: header.entry_count as usize,
            current_offset: core::mem::size_of::<BootFsHeader>(),
        })
    }
}

impl<'a> Iterator for BootFsScanner<'a> {
    type Item = BootFsEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_entry >= self.total_entries {
            return None;
        }
        let entry: BootFsEntry = self.vmo.read_struct(self.current_offset).ok()?;
        self.current_entry += 1;
        self.current_offset += core::mem::size_of::<BootFsEntry>();
        Some(entry)
    }
}

/// Resource Loader pattern to unpack and launch EL0 binaries
pub struct BootFsLoader<'a> {
    vmo: &'a Vmo,
}

impl<'a> BootFsLoader<'a> {
    pub const fn new(vmo: &'a Vmo) -> Self {
        Self { vmo }
    }

    /// Load and spawn a process directly from BootFS by path
    pub fn load_and_spawn(&self, path: &str) -> Result<u64, Status> {
        let scanner = BootFsScanner::new(self.vmo)?;
        
        for entry in scanner {
            let path_len = entry.path.iter().position(|&b| b == 0).unwrap_or(128);
            let file_path = core::str::from_utf8(&entry.path[..path_len])
                .map_err(|_| Status::InvalidArgs)?;

            if file_path == path {
                let file_offset = entry.offset as usize;
                let file_size = entry.size as usize;

                // Respect microkernel page alignment boundary constraints
                let aligned_off = file_offset & !(4096 - 1);
                let align_diff = file_offset - aligned_off;
                let aligned_sz = (file_size + align_diff + 4095) & !4095;

                let child_vmo = self.vmo.create_child(aligned_off, aligned_sz)?;
                let proc = Process::create(path)?;
                let pid = proc.load_binary(&child_vmo, path)?;
                return Ok(pid);
            }
        }
        Err(Status::NotFound)
    }
}
