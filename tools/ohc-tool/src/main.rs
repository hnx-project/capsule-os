use clap::{Parser, Subcommand};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;

const OHC_MAGIC: [u8; 4] = *b"OHC\0";
const OHC_VERSION: u16 = 0x0001;

#[repr(C)]
struct OhcHeader {
    magic: [u8; 4],
    version: u16,
    entry: u64,
    flags: u16,
    size: u32,
    checksum: u32,
    reserved: u32,
}

impl OhcHeader {
    fn new(size: u32, entry: u64) -> Self {
        OhcHeader {
            magic: OHC_MAGIC,
            version: OHC_VERSION,
            entry,
            flags: 0,
            size,
            checksum: 0,
            reserved: 0,
        }
    }

    fn to_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..6].copy_from_slice(&self.version.to_le_bytes());
        bytes[6..14].copy_from_slice(&self.entry.to_le_bytes());
        bytes[14..16].copy_from_slice(&self.flags.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.size.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.reserved.to_le_bytes());
        bytes
    }

    fn calculate_checksum(&self, payload: &[u8]) -> u32 {
        use crc32fast::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(payload);
        hasher.finalize()
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

    let mut header = OhcHeader::new(buffer.len() as u32, entry);
    header.checksum = header.calculate_checksum(&buffer);

    let mut output_file = File::create(output)?;
    output_file.write_all(&header.to_bytes())?;
    output_file.write_all(&buffer)?;

    println!("Packed {} bytes to {:?}", buffer.len(), output);
    println!("Entry: 0x{:016x}", entry);
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
        flags: u16::from_le_bytes([header_bytes[14], header_bytes[15]]),
        size: u32::from_le_bytes([header_bytes[16], header_bytes[17], header_bytes[18], header_bytes[19]]),
        checksum: u32::from_le_bytes([header_bytes[20], header_bytes[21], header_bytes[22], header_bytes[23]]),
        reserved: u32::from_le_bytes([header_bytes[24], header_bytes[25], header_bytes[26], header_bytes[27]]),
    };

    if &header.magic != b"OHC\0" {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid OHC magic"));
    }

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
        flags: u16::from_le_bytes([header_bytes[14], header_bytes[15]]),
        size: u32::from_le_bytes([header_bytes[16], header_bytes[17], header_bytes[18], header_bytes[19]]),
        checksum: u32::from_le_bytes([header_bytes[20], header_bytes[21], header_bytes[22], header_bytes[23]]),
        reserved: u32::from_le_bytes([header_bytes[24], header_bytes[25], header_bytes[26], header_bytes[27]]),
    };

    println!("OHC File Info");
    println!("=============");
    println!("Magic: {:?}", std::str::from_utf8(&header.magic).unwrap_or("INVALID"));
    println!("Version: 0x{:04x}", header.version);
    println!("Entry: 0x{:016x}", header.entry);
    println!("Flags: 0x{:04x}", header.flags);
    println!("Payload Size: {} bytes", header.size);
    println!("Checksum: 0x{:08x}", header.checksum);

    let file_size = fs::metadata(input)?.len();
    println!("File Size: {} bytes", file_size);
    println!("Header Size: 32 bytes");
    println!("Payload Offset: 32 bytes");

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
