use clap::Parser;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use ohlink_format::builder::OHLK_Builder;
use ohlink_format::entry::SegmentType;

#[derive(Parser, Debug)]
#[command(name = "ohlink-linker")]
#[command(about = "Standard Linker and Packer to compile standard OHLINK binaries")]
struct Cli {
    #[arg(short, long)]
    input: PathBuf,
    #[arg(short, long)]
    output: PathBuf,
    #[arg(short, long, default_value_t = 0x40080000)]
    entry: u64,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    let mut file = File::open(&cli.input)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Input file is empty"));
    }

    let mut is_elf = false;
    if buffer.len() > 64 && &buffer[0..4] == b"\x7fELF" {
        is_elf = true;
    }

    if !is_elf {
        // Create a completely clean new builder for fallback write (like kernel.raw flat binary)
        let mut fallback_builder = OHLK_Builder::new(1, 0);
        fallback_builder.add_segment(
            SegmentType::Text.to_u32(),
            1 | 2 | 4, // Read, Write, Execute (Kernel segments, unmask USER bit 8 to avoid privilege page fault loop!)
            &buffer,
            buffer.len() as u64,
        );
        let mut binary_data = fallback_builder.build().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("OHLINK Builder failed: {:?}", e))
        })?;
        
        // Correct the segment offset inside the header entry table to match the user's explicit --entry CLI argument for the flat binary
        let entry_bytes = cli.entry.to_le_bytes();
        binary_data[56..64].copy_from_slice(&entry_bytes); // Offset of Segment #0 is at 0x30 + 8 bytes = 56..64

        // Recalculate CRC32 IEEE of the entire final binary with the checksum bytes zeroed
        binary_data[28..32].copy_from_slice(&[0, 0, 0, 0]);
        let checksum_val = ohlink_format::crc32::crc32_ieee(&binary_data);
        binary_data[28..32].copy_from_slice(&checksum_val.to_le_bytes());

        let mut output_file = File::create(&cli.output)?;
        output_file.write_all(&binary_data)?;
    } else {
        // Input is ELF. To prevent 4 KiB page table translation faults on ARM64 due to segment overlaps (Data and Text residing on same 4KB page), 
        // we merge all loaded segments into a single, cohesive OHLK segment.
        let mut merged_payload = Vec::new();
        let mut merged_size = 0u64;
        let mut lowest_vaddr = u64::MAX;

        // Parse ELF64 header
        let e_phoff = u64::from_le_bytes([
            buffer[32], buffer[33], buffer[34], buffer[35],
            buffer[36], buffer[37], buffer[38], buffer[39],
        ]) as usize;
        let e_phnum = u16::from_le_bytes([buffer[56], buffer[57]]) as usize;
        let e_phentsize = u16::from_le_bytes([buffer[54], buffer[55]]) as usize;
        let mut actual_entry = u64::from_le_bytes([
            buffer[24], buffer[25], buffer[26], buffer[27],
            buffer[28], buffer[29], buffer[30], buffer[31],
        ]);

        // Accumulate segment payloads
        for i in 0..e_phnum {
            let offset = e_phoff + i * e_phentsize;
            if offset + 56 > buffer.len() {
                break;
            }
            let p_type = u32::from_le_bytes([
                buffer[offset], buffer[offset + 1], buffer[offset + 2], buffer[offset + 3],
            ]);

            if p_type == 1 {
                let p_offset = u64::from_le_bytes([
                    buffer[offset + 8], buffer[offset + 9], buffer[offset + 10], buffer[offset + 11],
                    buffer[offset + 12], buffer[offset + 13], buffer[offset + 14], buffer[offset + 15],
                ]) as usize;
                let p_vaddr = u64::from_le_bytes([
                    buffer[offset + 16], buffer[offset + 17], buffer[offset + 18], buffer[offset + 19],
                    buffer[offset + 20], buffer[offset + 21], buffer[offset + 22], buffer[offset + 23],
                ]);
                let p_filesz = u64::from_le_bytes([
                    buffer[offset + 32], buffer[offset + 33], buffer[offset + 34], buffer[offset + 35],
                    buffer[offset + 36], buffer[offset + 37], buffer[offset + 38], buffer[offset + 39],
                ]) as usize;

                if p_filesz > 0 && p_offset + p_filesz <= buffer.len() {
                    let segment_payload = &buffer[p_offset..p_offset + p_filesz];
                    merged_payload.extend_from_slice(segment_payload);
                    merged_size += p_filesz as u64;
                    if p_vaddr < lowest_vaddr && p_vaddr > 0 {
                        lowest_vaddr = p_vaddr;
                    }
                }
            }
        }

        // Export a single, fully integrated Text (executable and readable) segment conforming to standard format
        let mut final_builder = OHLK_Builder::new(1, 0);
        final_builder.add_segment(
            SegmentType::Text.to_u32(),
            1 | 2 | 4 | 8, // Read, Write, Execute, User space segment permissions
            &merged_payload,
            merged_payload.len() as u64, // Exact payload size
        );

        let mut binary_data = final_builder.build().map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("OHLINK Builder failed: {:?}", e))
        })?;

        // Patch Segment #0 fields:
        // Offset of Segment #0 starts at 0x30 (48 bytes).
        // Standard OHLK_Entry layout:
        // Offset 0..4 (u32): ty
        // Offset 4..8 (u32): flags
        // Offset 8..16 (u64): offset (set this to actual OHLINK file payload offset, which is 80 (0x50) in OHLINK, so parser.get_segment_data(entry) works correctly pre-alloc!)
        // Offset 16..24 (u64): file_size (set this to actual payload size)
        // Offset 24..32 (u64): mem_size (set this to actual payload size)
        
        let file_offset_bytes = 80u64.to_le_bytes(); // Segment data payload begins exactly at offset 80 (0x50) in OHLINK!
        binary_data[56..64].copy_from_slice(&file_offset_bytes); // offset of Segment #0 inside OHLINK binary file!

        let size_bytes = (merged_payload.len() as u64).to_le_bytes();
        binary_data[64..72].copy_from_slice(&size_bytes); // file_size of Segment #0 (actual bytes to read from file!)
        binary_data[72..80].copy_from_slice(&size_bytes); // mem_size of Segment #0 in memory

        let relative_entry = if actual_entry >= lowest_vaddr && lowest_vaddr != u64::MAX {
            actual_entry - lowest_vaddr
        } else {
            0
        };
        let relative_entry_bytes = relative_entry.to_le_bytes();
        binary_data[36..44].copy_from_slice(&relative_entry_bytes); // Save relative entry point offset inside reserved field!

        // Recalculate whole file checksum
        binary_data[28..32].copy_from_slice(&[0, 0, 0, 0]);
        let checksum_val = ohlink_format::crc32::crc32_ieee(&binary_data);
        binary_data[28..32].copy_from_slice(&checksum_val.to_le_bytes());

        let mut output_file = File::create(&cli.output)?;
        output_file.write_all(&binary_data)?;
    }

    println!("OHLINK Linker generated standard OHLK binary successfully at {:?}", cli.output);
    Ok(())
}