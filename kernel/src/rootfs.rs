const MAGIC: &[u8; 8] = b"HNXF_VFS";
const MAX_PATH_LEN: usize = 128;

pub static ROOTFS_IMAGE: &[u8] = include_bytes!("../files/rootfs.img");

pub fn get_file(path: &str) -> Option<&'static [u8]> {
    if ROOTFS_IMAGE.len() < 16 {
        return None;
    }
    
    // Check Magic
    if &ROOTFS_IMAGE[0..8] != MAGIC {
        return None;
    }
    
    // Read count
    let mut count_bytes = [0u8; 8];
    count_bytes.copy_from_slice(&ROOTFS_IMAGE[8..16]);
    let count = u64::from_le_bytes(count_bytes);
    
    let mut current_offset = 16;
    for _ in 0..count {
        if current_offset + 144 > ROOTFS_IMAGE.len() {
            break;
        }
        
        let path_start = current_offset;
        let path_end = path_start + MAX_PATH_LEN;
        
        // Find null terminator for path
        let mut actual_len = 0;
        for i in 0..MAX_PATH_LEN {
            if ROOTFS_IMAGE[path_start + i] == 0 {
                actual_len = i;
                break;
            }
        }
        
        let file_path = core::str::from_utf8(&ROOTFS_IMAGE[path_start..path_start + actual_len]).unwrap_or("");
        
        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&ROOTFS_IMAGE[path_end..path_end + 8]);
        let file_offset = u64::from_le_bytes(offset_bytes) as usize;
        
        let mut size_bytes = [0u8; 8];
        size_bytes.copy_from_slice(&ROOTFS_IMAGE[path_end + 8..path_end + 16]);
        let file_size = u64::from_le_bytes(size_bytes) as usize;
        
        crate::log_info!("ROOTFS", "Found entry: '{}', offset={}, size={}", file_path, file_offset, file_size);
        
        if file_path == path {
            if file_offset + file_size <= ROOTFS_IMAGE.len() {
                let bytes = &ROOTFS_IMAGE[file_offset..file_offset + file_size];
                crate::log_info!("ROOTFS", "Matched file: '{}', size={}, image_addr=0x{:x}, slice_addr=0x{:x}", path, file_size, ROOTFS_IMAGE.as_ptr() as usize, bytes.as_ptr() as usize);
                return Some(bytes);
            }
        }
        
        current_offset += 144;
    }
    
    None
}
