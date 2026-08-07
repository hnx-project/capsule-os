pub mod build_bootloader;
pub mod build_kernel;

use crate::config::{Resolved, get_parsed_version, BUILD_TEMP_RESOURCE};
use crate::platform::Platform;

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

pub fn build(resolved: &Resolved, plat: &Platform, _generate_dist: bool) -> Result<(), String> {
    println!(
        "{}    Building{} {} Ecosystem ({})",
        BOLD_CYAN, RESET, resolved.root.project.name, plat.arch
    );

    let v = get_parsed_version(&resolved.root);

    // Calculate total compilation steps dynamically based on sub-step weights
    let mut total_steps = 0;
    if resolved.build.bootloader.is_some() { total_steps += 2; } // Compiling + Extracting
    if resolved.build.kernel.is_some() { total_steps += 4; }     // Compiling + Linking + Extracting + Packing
    total_steps += resolved.build.libraries.len() * 2;           // Compiling + Extracting per library
    total_steps += resolved.build.pillsmod.len() * 2;            // Compiling + Packing per pill
    total_steps += resolved.build.services.len() * 2;            // Compiling + Packing per service
    total_steps += resolved.build.apps.len() * 2;                // Compiling + Packing per app

    let mut current_step = 1;

    // Run Bootloader build
    build_bootloader::build_bootloader(resolved, plat, &v, &mut current_step, total_steps as u32)?;

    // Run Microkernel build
    build_kernel::build_kernel(resolved, plat, &v, &mut current_step, total_steps as u32)?;

    // Print size summary
    print_sizes();

    println!(
        "\n{}     Success{} {} built successfully!\n",
        BOLD_GREEN, RESET, resolved.root.project.name
    );

    Ok(())
}

fn print_sizes() {
    let print_file_size = |label: &str, path: &str| {
        if let Ok(meta) = std::fs::metadata(path) {
            let size_kb = meta.len() as f64 / 1024.0;
            println!("  {}: [{:.1} KB]", label, size_kb);
        }
    };

    let boot_force = format!("{}/boot/BOOT_FORCE", BUILD_TEMP_RESOURCE);
    let boot_efi = format!("{}/EFI/BOOT/BOOTAA64.EFI", BUILD_TEMP_RESOURCE);
    let hnx_core = format!("{}/boot/HNXCore", BUILD_TEMP_RESOURCE);
    print_file_size("BOOT_FORCE", &boot_force);
    print_file_size("BOOTAA64.EFI", &boot_efi);
    print_file_size("HNXCore", &hnx_core);
}
