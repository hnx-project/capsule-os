/// Unified Service Descriptor shared between kernel-space and user-space libraries.
#[derive(Debug, Clone, Copy)]
pub struct ServiceDescriptor<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub bootstrap_vmo_handle_index: Option<u32>,
}

pub const MAGIC_BOOTFS: &[u8; 8] = b"HNXF_VFS";

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct BootFsHeader {
    pub magic: [u8; 8],
    pub entry_count: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct BootFsEntry {
    pub path: [u8; 128],
    pub offset: u64,
    pub size: u64,
}

/// Helper structure for common OS memory range and page alignments
pub struct AlignmentHelper;

impl AlignmentHelper {
    pub fn calculate_aligned_range(offset: usize, size: usize) -> (usize, usize, usize) {
        let aligned_off = offset & !(4096 - 1);
        let align_diff = offset - aligned_off;
        let aligned_sz = (size + align_diff + 4095) & !4095;
        (aligned_off, align_diff, aligned_sz)
    }
}
