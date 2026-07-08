use ohlink_format::parser::OHLK_Parser;

fn main() {
    let file_path = "examples/hello_hnx/hello_hnx.ohlk";
    println!("Loading OHLINK target file from: {}", file_path);

    let data = std::fs::read(file_path).expect("Failed to read OHLINK binary");
    
    // Attempt parse
    let parser = OHLK_Parser::new(&data).expect("Failed to parse and validate OHLINK file headers or checksum!");
    let header = parser.header();

    println!("--- OHLINK Binary parsed successfully! ---");
    println!("Magic: 0x{:08X} (\"OHLK\")", header.magic);
    println!("Version: {}.{}", header.version_major, header.version_minor);
    println!("Architecture: {} (1=ARM64)", header.arch);
    println!("File size: {} bytes", header.file_size);
    println!("Header count (entries): {}", header.header_count);
    println!("Checksum: 0x{:08X}", header.checksum);
    println!("Flags: 0x{:08X}", header.flags);

    println!("--- Segment Table Entries ---");
    for (i, entry) in parser.entries().enumerate() {
        println!("  Entry[{}]: type=0x{:08X}, flags=0x{:X}, offset=0x{:X}, file_size={}, mem_size={}",
            i, entry.ty, entry.flags, entry.offset, entry.file_size, entry.mem_size
        );
        let seg_data = parser.get_segment_data(&entry).expect("Failed to read segment payload");
        println!("    Payload bytes: {:?}", seg_data);
    }
}
