use clap::Parser;
use std::fs::{File};
use std::io::{Read, Write};
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
    #[arg(short, long, default_value_t = 0)]
    entry: u64,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    let mut file = File::open(&cli.input)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Input file is empty"));
    }

    let mut is_elf = false;
    if buffer.len() > 64 && &buffer[0..4] == b"\x7fELF" {
        is_elf = true;
    }

    if !is_elf {
        let mut builder = OHLK_Builder::new(1, 1, 0, cli.entry);
        builder.add_segment(
            SegmentType::Text.to_u32(),
            1 | 2 | 4,
            &buffer,
            buffer.len() as u64,
            0,
            4096,
        );
        let binary_data = builder.build().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("OHLINK Builder failed: {:?}", e))
        })?;

        let mut output_file = File::create(&cli.output)?;
        output_file.write_all(&binary_data)?;
    } else {
        let mut merged_payload = Vec::new();
        let mut lowest_vaddr = u64::MAX;

        let e_phoff = u64::from_le_bytes([
            buffer[32], buffer[33], buffer[34], buffer[35],
            buffer[36], buffer[37], buffer[38], buffer[39],
        ]) as usize;
        let e_phnum = u16::from_le_bytes([buffer[56], buffer[57]]) as usize;
        let e_phentsize = u16::from_le_bytes([buffer[54], buffer[55]]) as usize;
        let e_entry = u64::from_le_bytes([
            buffer[24], buffer[25], buffer[26], buffer[27],
            buffer[28], buffer[29], buffer[30], buffer[31],
        ]);

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

                    if p_vaddr < lowest_vaddr && p_vaddr > 0 {
                        lowest_vaddr = p_vaddr;
                    }
                }
            }
        }

        let entry_point = if cli.entry != 0 {
            cli.entry
        } else {
            e_entry
        };

        let mut builder = OHLK_Builder::new(1, 1, 0, entry_point);
        builder.add_segment(
            SegmentType::Text.to_u32(),
            1 | 2 | 4 | 8,
            &merged_payload,
            merged_payload.len() as u64,
            lowest_vaddr,
            4096,
        );

        let binary_data = builder.build().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("OHLINK Builder failed: {:?}", e))
        })?;

        let mut output_file = File::create(&cli.output)?;
        output_file.write_all(&binary_data)?;
    }

    println!("OHLINK Linker generated standard OHLK binary successfully at {:?}", cli.output);
    Ok(())
}
