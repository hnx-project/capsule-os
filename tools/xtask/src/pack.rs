use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

const MAGIC: &[u8; 8] = b"HNXF_VFS";
const MAX_PATH_LEN: usize = 128;

#[derive(Debug)]
struct FileEntry {
    path: String,
    data: Vec<u8>,
}

pub fn pack_rootfs(staging_dir: &str, output_img: &str) -> io::Result<()> {
    let mut entries = Vec::new();
    collect_files(Path::new(staging_dir), Path::new(staging_dir), &mut entries)?;

    let mut out_file = File::create(output_img)?;

    // 1. Write Superblock
    out_file.write_all(MAGIC)?;
    let count = entries.len() as u64;
    out_file.write_all(&count.to_le_bytes())?;

    // Calculate start offset of data section
    // Superblock: 8 + 8 = 16 bytes
    // Entry: 128 (path) + 8 (offset) + 8 (size) = 144 bytes per entry
    // 16 + N * 144 is mathematically guaranteed to be 16-byte aligned (16 and 144 are multiples of 16)
    let mut current_offset = 16 + (entries.len() as u64 * 144);

    // 2. Write Entries (header metadata)
    for entry in &mut entries {
        let mut path_bytes = [0u8; MAX_PATH_LEN];
        let entry_path = entry.path.replace("\\", "/"); // Normalize paths to forward slashes
        let src_bytes = entry_path.as_bytes();
        let copy_len = src_bytes.len().min(MAX_PATH_LEN - 1);
        path_bytes[..copy_len].copy_from_slice(&src_bytes[..copy_len]);

        out_file.write_all(&path_bytes)?;
        out_file.write_all(&current_offset.to_le_bytes())?;

        let size = entry.data.len() as u64;
        out_file.write_all(&size.to_le_bytes())?;

        // 16-byte align the offset of the NEXT file's data by padding this file's data at the end
        current_offset += size;
        let padding = (16 - (current_offset % 16)) % 16;
        if padding > 0 {
            entry.data.resize(entry.data.len() + padding as usize, 0);
        }
        current_offset += padding;
    }

    // 3. Write Data blocks
    for entry in &entries {
        out_file.write_all(&entry.data)?;
    }

    Ok(())
}

fn collect_files(root: &Path, current: &Path, entries: &mut Vec<FileEntry>) -> io::Result<()> {
    if current.is_dir() {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            collect_files(root, &path, entries)?;
        }
    } else if current.is_file() {
        // Calculate relative path from staging_dir root
        let rel_path = current.strip_prefix(root).unwrap();
        let rel_str = rel_path.to_string_lossy().into_owned();

        let mut data = Vec::new();
        let mut f = File::open(current)?;
        f.read_to_end(&mut data)?;

        entries.push(FileEntry {
            path: rel_str,
            data,
        });
    }
    Ok(())
}
