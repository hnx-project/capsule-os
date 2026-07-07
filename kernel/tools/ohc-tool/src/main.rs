use clap::{Parser, Subcommand};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;

const OHC_MAGIC: [u8; 4] = *b"OHLK";
const OHC_VERSION: u16 = 0x0002;

#[repr(C)]
struct OhcHeader {
    magic: [u8; 4],
    version: u16,
    entry: u64,
    segment_count: u16,
    flags: u16,
    size: u32,
    checksum: u32,
}

impl OhcHeader {
    fn new(size: u32, entry: u64, segment_count: u16) -> Self {
        OhcHeader {
            magic: OHC_MAGIC,
            version: OHC_VERSION,
            entry,
            segment_count,
            flags: 0,
            size,
            checksum: 0,
        }
    }

    fn to_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..6].copy_from_slice(&self.version.to_le_bytes());
        bytes[6..14].copy_from_slice(&self.entry.to_le_bytes());
        bytes[14..16].copy_from_slice(&self.segment_count.to_le_bytes());
        bytes[16..18].copy_from_slice(&self.flags.to_le_bytes());
        bytes[18..22].copy_from_slice(&self.size.to_le_bytes());
        bytes[22..26].copy_from_slice(&self.checksum.to_le_bytes());
        bytes
    }

    fn calculate_checksum(&self, payload: &[u8]) -> u32 {
        use crc32fast::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(payload);
        hasher.finalize()
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct SegmentDescriptor {
    virt_addr: u64,
    file_offset: u64,
    size: u32,
    flags: u32,
}

impl SegmentDescriptor {
    fn to_bytes(&self) -> [u8; 24] {
        let mut bytes = [0u8; 24];
        bytes[0..8].copy_from_slice(&self.virt_addr.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.file_offset.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.size.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }
}

#[derive(Parser, Debug)]
#[command(name = "ohc-tool")]
#[command(about = "Pack and unpack .ohc capsule files")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Pack {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(short, long, default_value_t = 0x40080000)]
        entry: u64,
    },
    Unpack {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    Info {
        #[arg(short, long)]
        input: PathBuf,
    },
}

fn pack(input: &PathBuf, output: &PathBuf, entry: u64) -> io::Result<()> {
    let mut file = File::open(input)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    if buffer.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Input file is empty"));
    }

    let mut segments = Vec::new();
    let mut payload = Vec::new();
    let mut actual_entry = entry;

    // Check if input is an ELF executable
    if buffer.len() > 64 && &buffer[0..4] == b"\x7fELF" {
        // Parse ELF64 header
        let e_phoff = u64::from_le_bytes([
            buffer[32], buffer[33], buffer[34], buffer[35],
            buffer[36], buffer[37], buffer[38], buffer[39],
        ]) as usize;
        let e_phnum = u16::from_le_bytes([buffer[56], buffer[57]]) as usize;
        let e_phentsize = u16::from_le_bytes([buffer[54], buffer[55]]) as usize;
        actual_entry = u64::from_le_bytes([
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

            // PT_LOAD segment
            if p_type == 1 {
                let p_flags = u32::from_le_bytes([
                    buffer[offset + 4], buffer[offset + 5], buffer[offset + 6], buffer[offset + 7],
                ]);
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
                    let current_payload_offset = payload.len();
                    payload.extend_from_slice(&buffer[p_offset..p_offset + p_filesz]);

                    segments.push(SegmentDescriptor {
                        virt_addr: p_vaddr,
                        file_offset: current_payload_offset as u64,
                        size: p_filesz as u32,
                        flags: p_flags,
                    });
                }
            }
        }
    }

    // Fallback if not an ELF, or no LOAD segments found
    if segments.is_empty() {
        payload = buffer;
        segments.push(SegmentDescriptor {
            virt_addr: actual_entry,
            file_offset: 0,
            size: payload.len() as u32,
            flags: 7, // Read, Write, Execute
        });
    }

    let mut header = OhcHeader::new(payload.len() as u32, actual_entry, segments.len() as u16);
    header.checksum = header.calculate_checksum(&payload);

    let mut output_file = File::create(output)?;
    output_file.write_all(&header.to_bytes())?;
    for seg in &segments {
        output_file.write_all(&seg.to_bytes())?;
    }
    output_file.write_all(&payload)?;

    println!("Packed {} segments (payload: {} bytes) to {:?}", segments.len(), payload.len(), output);
    println!("Entry: 0x{:016x}", actual_entry);
    println!("Checksum: 0x{:08x}", header.checksum);

    Ok(())
}

fn unpack(input: &PathBuf, output: &PathBuf) -> io::Result<()> {
    let mut file = File::open(input)?;
    let mut header_bytes = [0u8; 32];
    file.read_exact(&mut header_bytes)?;

    let header = OhcHeader {
        magic: [header_bytes[0], header_bytes[1], header_bytes[2], header_bytes[3]],
        version: u16::from_le_bytes([header_bytes[4], header_bytes[5]]),
        entry: u64::from_le_bytes([
            header_bytes[6], header_bytes[7], header_bytes[8], header_bytes[9],
            header_bytes[10], header_bytes[11], header_bytes[12], header_bytes[13],
        ]),
        segment_count: u16::from_le_bytes([header_bytes[14], header_bytes[15]]),
        flags: u16::from_le_bytes([header_bytes[16], header_bytes[17]]),
        size: u32::from_le_bytes([header_bytes[18], header_bytes[19], header_bytes[20], header_bytes[21]]),
        checksum: u32::from_le_bytes([header_bytes[22], header_bytes[23], header_bytes[24], header_bytes[25]]),
    };

    if &header.magic != b"OHLK" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid OHLINK magic"));
    }

    // Skip Segment Descriptors
    let descriptors_size = (header.segment_count as usize) * 24;
    let mut unused_desc = vec![0u8; descriptors_size];
    file.read_exact(&mut unused_desc)?;

    let mut payload = Vec::new();
    file.read_to_end(&mut payload)?;

    let calculated_checksum = header.calculate_checksum(&payload);
    if calculated_checksum != header.checksum {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Checksum mismatch: expected 0x{:08x}, got 0x{:08x}", header.checksum, calculated_checksum)));
    }

    let mut output_file = File::create(output)?;
    output_file.write_all(&payload)?;

    println!("Unpacked {} bytes to {:?}", header.size, output);

    Ok(())
}

fn info(input: &PathBuf) -> io::Result<()> {
    let mut file = File::open(input)?;
    let mut header_bytes = [0u8; 32];
    file.read_exact(&mut header_bytes)?;

    let header = OhcHeader {
        magic: [header_bytes[0], header_bytes[1], header_bytes[2], header_bytes[3]],
        version: u16::from_le_bytes([header_bytes[4], header_bytes[5]]),
        entry: u64::from_le_bytes([
            header_bytes[6], header_bytes[7], header_bytes[8], header_bytes[9],
            header_bytes[10], header_bytes[11], header_bytes[12], header_bytes[13],
        ]),
        segment_count: u16::from_le_bytes([header_bytes[14], header_bytes[15]]),
        flags: u16::from_le_bytes([header_bytes[16], header_bytes[17]]),
        size: u32::from_le_bytes([header_bytes[18], header_bytes[19], header_bytes[20], header_bytes[21]]),
        checksum: u32::from_le_bytes([header_bytes[22], header_bytes[23], header_bytes[24], header_bytes[25]]),
    };

    println!("OHLINK File Info");
    println!("================");
    println!("Magic: {:?}", std::str::from_utf8(&header.magic).unwrap_or("INVALID"));
    println!("Version: 0x{:04x}", header.version);
    println!("Entry: 0x{:016x}", header.entry);
    println!("Segment Count: {}", header.segment_count);
    println!("Flags: 0x{:04x}", header.flags);
    println!("Payload Size: {} bytes", header.size);
    println!("Checksum: 0x{:08x}", header.checksum);

    // Read and print Segment Descriptors
    for i in 0..header.segment_count {
        let mut desc_bytes = [0u8; 24];
        file.read_exact(&mut desc_bytes)?;
        let desc = SegmentDescriptor {
            virt_addr: u64::from_le_bytes([
                desc_bytes[0], desc_bytes[1], desc_bytes[2], desc_bytes[3],
                desc_bytes[4], desc_bytes[5], desc_bytes[6], desc_bytes[7],
            ]),
            file_offset: u64::from_le_bytes([
                desc_bytes[8], desc_bytes[9], desc_bytes[10], desc_bytes[11],
                desc_bytes[12], desc_bytes[13], desc_bytes[14], desc_bytes[15],
            ]),
            size: u32::from_le_bytes([desc_bytes[16], desc_bytes[17], desc_bytes[18], desc_bytes[19]]),
            flags: u32::from_le_bytes([desc_bytes[20], desc_bytes[21], desc_bytes[22], desc_bytes[23]]),
        };
        println!("  Segment #{}: virt={:016x}, offset={}, size={}, flags={}", i, desc.virt_addr, desc.file_offset, desc.size, desc.flags);
    }

    let file_size = fs::metadata(input)?.len();
    println!("File Size: {} bytes", file_size);

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Pack { input, output, entry } => pack(&input, &output, entry),
        Command::Unpack { input, output } => unpack(&input, &output),
        Command::Info { input } => info(&input),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
