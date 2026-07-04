use std::process::Command;

use crate::output::run_silent;
use crate::platform::Platform;
use crate::toolchain::{find_objcopy, find_rust_lld};

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

pub fn build(plat: &Platform) -> Result<(), String> {
    println!("{}    Building{} CapsuleOS Ecosystem ({})", BOLD_CYAN, RESET, plat.arch);

    build_userspace_loader(plat)?;
    pack_loader(plat)?;
    build_kernel(plat)?;
    link_kernel(plat)?;
    extract_kernel_raw(plat)?;
    pack_kernel_ohc(plat)?;
    build_bootloader(plat)?;
    extract_bootloader_bin(plat)?;
    print_build_summary(plat);

    println!("\n{}     Success{} CapsuleOS built successfully!\n", BOLD_GREEN, RESET);
    Ok(())
}

fn build_userspace_loader(plat: &Platform) -> Result<(), String> {
    print!("{}  Building{} loader (EL0)...", BOLD_GREEN, RESET);
    let userspace_target = format!("std/targets/{}-unknown-capsule.json", plat.arch);
    let result = run_silent(
        Command::new("cargo")
            .args(["+nightly", "build", "--release", "-p", "hnx-loader", "--target", &userspace_target, "-Z", "build-std=core,alloc,panic_abort", "-Z", "json-target-spec"]),
        || { println!("\r{}  Building{} loader (EL0)... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to build loader".to_string()) } else { Ok(()) }
}

fn pack_loader(plat: &Platform) -> Result<(), String> {
    print!("{}  Packing{} loader.ohc...", BOLD_GREEN, RESET);
    let loader_elf = format!("build/target/{}-unknown-capsule/release/loader", plat.arch);
    let result = run_silent(
        Command::new("cargo")
            .args(["run", "--manifest-path", "kernel/Cargo.toml", "-p", "ohc-tool", "--", "pack", "--input", &loader_elf, "--output", "kernel/files/loader.ohc", "--entry", "65536"]),
        || { println!("\r{}  Packing{} loader.ohc... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to pack loader".to_string()) } else { Ok(()) }
}

fn build_kernel(plat: &Platform) -> Result<(), String> {
    print!("{}  Building{} hnx-core (kernel)...", BOLD_GREEN, RESET);
    let result = run_silent(
        Command::new("cargo")
            .args(["build", "--target", plat.rust_target, "-p", "kernel", "--release"])
            .current_dir("kernel"),
        || { println!("\r{}  Building{} hnx-core (kernel)... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to build kernel".to_string()) } else { Ok(()) }
}

fn link_kernel(plat: &Platform) -> Result<(), String> {
    std::fs::create_dir_all("dist/kernel").map_err(|e| e.to_string())?;
    print!("{}  Linking{} dist/kernel/kernel.elf...", BOLD_GREEN, RESET);
    let lld = find_rust_lld();
    let result = run_silent(
        Command::new(&lld)
            .args(["-flavor", "gnu", "-m", plat.ld_emulation, "--gc-sections", "--whole-archive", "-T", plat.linker_script, &format!("kernel/build/target/{}/release/libkernel.a", plat.rust_target), "--no-whole-archive", "-o", "dist/kernel/kernel.elf"]),
        || { println!("\r{}  Linking{} dist/kernel/kernel.elf... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to link kernel".to_string()) } else { Ok(()) }
}

fn extract_kernel_raw(plat: &Platform) -> Result<(), String> {
    print!("{}  Extracting{} dist/kernel/kernel.raw...", BOLD_GREEN, RESET);
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy)
            .args(["-O", "binary", "dist/kernel/kernel.elf", "dist/kernel/kernel.raw"]),
        || { println!("\r{}  Extracting{} dist/kernel/kernel.raw... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to extract raw binary".to_string()) } else { Ok(()) }
}

fn pack_kernel_ohc(plat: &Platform) -> Result<(), String> {
    print!("{}  Packing{} dist/kernel/hnxcore.ohc...", BOLD_GREEN, RESET);
    let result = run_silent(
        Command::new("cargo")
            .args(["run", "--manifest-path", "kernel/Cargo.toml", "-p", "ohc-tool", "--", "pack", "--input", "dist/kernel/kernel.raw", "--output", "dist/kernel/hnxcore.ohc", "--entry", plat.kernel_entry]),
        || { println!("\r{}  Packing{} dist/kernel/hnxcore.ohc... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to pack kernel".to_string()) } else { Ok(()) }
}

fn build_bootloader(plat: &Platform) -> Result<(), String> {
    print!("{}  Building{} capsule-bootloader...", BOLD_GREEN, RESET);
    let result = run_silent(
        Command::new("cargo")
            .args(["build", "--release", "-p", "capsule-bootloader", "--target", plat.rust_target]),
        || { println!("\r{}  Building{} capsule-bootloader... Done", BOLD_GREEN, RESET); }
    );
    if !result.success { Err("failed to build bootloader".to_string()) } else { Ok(()) }
}

fn extract_bootloader_bin(plat: &Platform) -> Result<(), String> {
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy)
            .args(["-O", "binary", &format!("build/target/{}/release/capsule-bootloader", plat.rust_target), &format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target)]),
        || {}
    );
    if !result.success { Err("failed to extract bootloader".to_string()) } else { Ok(()) }
}

fn print_build_summary(plat: &Platform) {
    let print_size = |label: &str, path: &str| {
        if let Ok(meta) = std::fs::metadata(path) {
            let size_kb = meta.len() as f64 / 1024.0;
            println!("  {}: [{:.1} KB]", label, size_kb);
        }
    };
    print_size("hnxcore.ohc", "dist/kernel/hnxcore.ohc");
    print_size("capsule-bootloader.bin", &format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target));
}
