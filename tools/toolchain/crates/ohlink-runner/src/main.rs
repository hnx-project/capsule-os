pub mod mmu;
pub mod cpu;

use mmu::Mmu;
use cpu::Cpu;
use ohlink_format::parser::OHLK_Parser;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("OHLINK Runner (User-space AArch64 Emulator)");
        println!("Usage: ohlink-runner <file.ohlk>");
        std::process::exit(1);
    }

    let file_path = &args[1];
    if !Path::new(file_path).exists() {
        eprintln!("Error: Target file '{}' does not exist.", file_path);
        std::process::exit(1);
    }

    // Read the binary file
    let binary_data = match fs::read(file_path) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error: Failed to read OHLINK binary: {:?}", e);
            std::process::exit(1);
        }
    };

    // Parse format headers and verify checksums
    let parser = match OHLK_Parser::new(&binary_data) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: Failed to parse and validate OHLINK format: {:?}", e);
            std::process::exit(1);
        }
    };

    let header = parser.header();
    if header.arch != 1 {
        eprintln!("Error: Target file architecture is not AArch64 (Arch: {}). Only ARM64 execution is supported.", header.arch);
        std::process::exit(1);
    }

    // Initialize virtual physical memory (MMU)
    let mut mmu = Mmu::new();

    // Map and load text/data segments into virtual RAM space
    let mut entry_point = Mmu::RAM_DEFAULT_BASE;
    let mut has_text = false;

    for (i, entry) in parser.entries().enumerate() {
        let seg_data = match parser.get_segment_data(&entry) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error: Failed to read segment [{}] payload: {:?}", i, e);
                std::process::exit(1);
            }
        };

        // Determine mapped target load address.
        // For simple execution, we map the first TYPE_TEXT segment directly to standard QEMU Virt RAM base.
        if entry.ty == ohlink_format::SegmentType::Text.to_u32() {
            entry_point = Mmu::RAM_DEFAULT_BASE;
            mmu.load_segment(entry_point, seg_data);
            has_text = true;
        } else if entry.ty == ohlink_format::SegmentType::Data.to_u32() || entry.ty == ohlink_format::SegmentType::Rodata.to_u32() {
            // Place other segments offset by 1MB increments inside the 16MB virtual RAM
            let data_load_addr = Mmu::RAM_DEFAULT_BASE + 0x100000; // 1MB offset
            mmu.load_segment(data_load_addr, seg_data);
        }
    }

    if !has_text {
        eprintln!("Error: No executable code (.text) segment found in the target OHLINK binary.");
        std::process::exit(1);
    }

    // Set Stack Pointer to high RAM offset (e.g. 8MB offset)
    let stack_pointer = Mmu::RAM_DEFAULT_BASE + (8 * 1024 * 1024);

    // Initialize CPU State
    let mut cpu = Cpu::new(entry_point, stack_pointer);

    // Start emulation execution loop
    let mut cycles = 0u64;
    let max_cycles = 100_000u64; // Fallback threshold to prevent infinite hangs

    while cpu.step(&mut mmu) {
        cycles += 1;
        if cycles >= max_cycles {
            println!("\n[Runner Info] Exceeded instruction limit threshold ({} cycles). Suspending execution.", max_cycles);
            break;
        }
    }
}
