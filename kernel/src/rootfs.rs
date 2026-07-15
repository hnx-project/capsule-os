const MAGIC: &[u8; 8] = b"HNXF_VFS";
const MAX_PATH_LEN: usize = 128;

pub fn get_rootfs_image() -> &'static [u8] {
    unsafe {
        let va = crate::mm::mmu::pa_to_kernel_va(crate::BOOTFS_PHYS_ADDR);
        core::slice::from_raw_parts(va as *const u8, crate::BOOTFS_PHYS_SIZE)
    }
}

pub fn get_file(path: &str) -> Option<&'static [u8]> {
    let rootfs_image = get_rootfs_image();
    if rootfs_image.len() < 16 {
        return None;
    }

    // Check Magic
    if &rootfs_image[0..8] != MAGIC {
        return None;
    }

    // Read count
    let mut count_bytes = [0u8; 8];
    count_bytes.copy_from_slice(&rootfs_image[8..16]);
    let count = u64::from_le_bytes(count_bytes);

    let mut current_offset = 16;
    for _ in 0..count {
        if current_offset + 144 > rootfs_image.len() {
            break;
        }

        let path_start = current_offset;
        let path_end = path_start + MAX_PATH_LEN;

        // Find null terminator for path
        let mut actual_len = 0;
        for i in 0..MAX_PATH_LEN {
            if rootfs_image[path_start + i] == 0 {
                actual_len = i;
                break;
            }
        }

        let file_path =
            core::str::from_utf8(&rootfs_image[path_start..path_start + actual_len]).unwrap_or("");

        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&rootfs_image[path_end..path_end + 8]);
        let file_offset = u64::from_le_bytes(offset_bytes) as usize;

        let mut size_bytes = [0u8; 8];
        size_bytes.copy_from_slice(&rootfs_image[path_end + 8..path_end + 16]);
        let file_size = u64::from_le_bytes(size_bytes) as usize;

        // crate::log_info!("ROOTFS", "Found entry: '{}', offset={}, size={}", file_path, file_offset, file_size);

        if file_path == path {
            if file_offset + file_size <= rootfs_image.len() {
                let bytes = &rootfs_image[file_offset..file_offset + file_size];
                crate::log_info!(
                    "ROOTFS",
                    "Matched file: '{}', size={}, image_addr=0x{:x}, slice_addr=0x{:x}",
                    path,
                    file_size,
                    rootfs_image.as_ptr() as usize,
                    bytes.as_ptr() as usize
                );
                return Some(bytes);
            }
        }

        current_offset += 144;
    }

    None
}
