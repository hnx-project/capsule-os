use clap::Parser;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use ohlink_format::parser::OHLK_Parser;

#[derive(Parser, Debug)]
#[command(name = "ohlink-read")]
#[command(about = "Read and display OHLINK binary file information")]
struct Cli {
    #[arg(short, long)]
    input: PathBuf,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    let mut file = File::open(&cli.input)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Input file is empty"));
    }

    let parser = OHLK_Parser::new(&buffer).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Failed to parse OHLINK: {:?}", e))
    })?;

    let header = parser.header();

    println!("OHLINK Binary Information");
    println!("========================");
    println!();
    println!("Header:");
    println!("  Magic:          0x{:08X}", header.magic);
    println!("  Version:        {}.{}", header.version_major, header.version_minor);
    println!("  Endian:         {}", if header.endian == 0 { "Little" } else { "Big" });
    println!("  Architecture:   {}", arch_name(header.arch));
    println!("  File Type:      {}", file_type_name(header.file_type));
    println!("  Entry Count:    {}", header.header_count);
    println!("  Header Offset:  0x{:08X}", header.header_offset);
    println!("  Data Offset:    0x{:08X}", header.data_offset);
    println!("  File Size:      {} bytes", header.file_size);
    println!("  Checksum:       0x{:08X}", header.checksum);
    println!("  Flags:          0x{:08X}", header.flags);
    println!("  Entry Point:    0x{:016X}", header.entry_point);
    println!();

    println!("Segments/Entries:");
    println!("  {:<4} {:<10} {:<10} {:<18} {:<18} {:<12} {:<12} {:<10}",
             "Idx", "Type", "Flags", "FileOffset", "VirtAddr", "FileSize", "MemSize", "Align");
    println!("  {:-<4} {:-<10} {:-<10} {:-<18} {:-<18} {:-<12} {:-<12} {:-<10}",
             "", "", "", "", "", "", "", "");

    for i in 0..header.header_count {
        if let Ok(entry) = parser.get_entry(i) {
            let type_name = segment_type_name(entry.ty);
            let flags_str = format_flags(entry.flags);
            println!("  {:<4} {:<10} {:<10} 0x{:016X} 0x{:016X} {:<12} {:<12} 0x{:08X}",
                     i, type_name, flags_str, entry.file_offset, entry.virtual_address,
                     entry.file_size, entry.mem_size, entry.alignment);
        }
    }

    println!();
    println!("Hex Dump (first 256 bytes):");
    println!("  Offset  00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F  0123456789ABCDEF");
    println!("  {:-<80}", "");

    let dump_len = std::cmp::min(256, buffer.len());
    for row in (0..dump_len).step_by(16) {
        let end = std::cmp::min(row + 16, dump_len);
        let slice = &buffer[row..end];

        let hex_part: Vec<String> = slice.iter()
            .map(|b| format!("{:02X}", b))
            .collect();
        let hex_line = hex_part.join(" ");

        let ascii_part: String = slice.iter()
            .map(|b| if *b >= 0x20 && *b <= 0x7E { *b as char } else { '.' })
            .collect();

        println!("  {:04X}  {:<47}  {}", row, hex_line, ascii_part);
    }

    Ok(())
}

fn arch_name(arch: u8) -> &'static str {
    match arch {
        1 => "AArch64",
        2 => "x86_64",
        3 => "RISC-V64",
        _ => "Unknown",
    }
}

fn file_type_name(file_type: u16) -> &'static str {
    match file_type {
        1 => "Executable",
        2 => "Relocatable",
        3 => "Shared Library",
        4 => "Kernel Module",
        5 => "Firmware",
        6 => "Bootloader",
        _ => "Unknown",
    }
}

fn segment_type_name(ty: u32) -> &'static str {
    match ty {
        1 => "TEXT",
        2 => "DATA",
        3 => "RODATA",
        4 => "BSS",
        5 => "SYMTAB",
        6 => "STRTAB",
        7 => "RELOC",
        8 => "DYNAMIC",
        9 => "IMPORT",
        10 => "EXPORT",
        11 => "GOT",
        12 => "PLT",
        13 => "HASH",
        14 => "TLS",
        15 => "INIT",
        16 => "FINI",
        17 => "DEBUG",
        18 => "LINE",
        _ => "CUSTOM",
    }
}

fn format_flags(flags: u32) -> String {
    let mut s = String::new();
    if flags & 1 != 0 { s.push('R'); }
    if flags & 2 != 0 { s.push('W'); }
    if flags & 4 != 0 { s.push('X'); }
    if s.is_empty() { s.push('-'); }
    s
}
