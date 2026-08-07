use std::process::Command;
use std::path::Path;
use crate::config::{Resolved, ParsedVersion, resolve_path_placeholders, parse_cargo_toml};
use crate::platform::Platform;
use crate::output::run_silent;
use crate::code::toolchain::find_objcopy;

pub fn build_bootloader(
    resolved: &Resolved,
    plat: &Platform,
    v: &ParsedVersion,
    current_step: &mut u32,
    total_steps: u32,
) -> Result<(), String> {
    let boot = match &resolved.build.bootloader {
        Some(b) => b,
        None => return Ok(()),
    };

    use std::io::Write;

    // Dynamically detect bootloader package metadata from its Cargo.toml
    let (crate_name, bin_name) = parse_cargo_toml(&boot.path)?;
    let is_uefi = crate_name.contains("uefi") || boot.path.contains("uefi");
    
    let target_triple = if is_uefi {
        "aarch64-unknown-uefi"
    } else {
        &plat.rust_target
    };

    // Sub-step 1: Compiling
    let comp_step = *current_step;
    *current_step += 1;

    print!("  Compiling ({}/{}) {}...", comp_step, total_steps, crate_name);
    std::io::stdout().flush().unwrap();
    
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--release",
        "-p",
        &crate_name,
        "--target",
        target_triple,
    ]);
    
    let display_str = v.to_display_string();
    cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
    cmd.env("CAPSULEOS_VERSION", &display_str);

    let result = run_silent(&mut cmd, || {});
    if !result.success {
        println!(); // ensure newline on error
        return Err(format!("failed to build bootloader {}", crate_name));
    }
    println!("\r  Compiling ({}/{}) {}... DONE\x1B[K", comp_step, total_steps, crate_name);

    // Sub-step 2: Extracting (Bare-Metal) or Copying (UEFI)
    let ext_step = *current_step;
    *current_step += 1;

    let output_resolved = resolve_path_placeholders(&boot.output);
    let out_name = Path::new(&output_resolved)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("BOOT_FORCE");

    if let Some(parent) = Path::new(&output_resolved).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if is_uefi {
        print!("  Extracting ({}/{}) {}...", ext_step, total_steps, out_name);
        std::io::stdout().flush().unwrap();

        // Standard UEFI target binary is already PE32+ .efi format. Copy directly.
        let efi_src = format!("{}/{}/release/{}.efi", crate::config::BUILD_TARGET, target_triple, bin_name);
        
        if !Path::new(&efi_src).exists() {
            println!();
            return Err(format!("compiled UEFI artifact not found: {}", efi_src));
        }

        std::fs::copy(&efi_src, &output_resolved).map_err(|e| {
            format!("failed to copy UEFI bootloader: {}", e)
        })?;
        
        println!("\r  Extracting ({}/{}) {}... DONE\x1B[K", ext_step, total_steps, out_name);
    } else {
        print!("  Extracting ({}/{}) {}...", ext_step, total_steps, out_name);
        std::io::stdout().flush().unwrap();

        let elf_path = format!("{}/{}/release/{}", crate::config::BUILD_TARGET, target_triple, bin_name);
        
        let objcopy = find_objcopy();
        let result = run_silent(
            Command::new(&objcopy).args(["-O", "binary", &elf_path, &output_resolved]),
            || {},
        );

        if !result.success {
            println!(); // ensure newline on error
            return Err("failed to extract bootloader binary".to_string());
        }
        println!("\r  Extracting ({}/{}) {}... DONE\x1B[K", ext_step, total_steps, out_name);
    }

    // Auto-copy any DTB files under dtb/ to the virtual bootpack
    copy_dtb_files_if_exists()?;

    Ok(())
}

fn copy_dtb_files_if_exists() -> Result<(), String> {
    let src_dir = Path::new("dtb");
    if src_dir.exists() && src_dir.is_dir() {
        let dst_dir = Path::new(crate::config::BUILD_TEMP_RESOURCE).join("boot/dtb");
        std::fs::create_dir_all(&dst_dir).map_err(|e| e.to_string())?;

        for entry in std::fs::read_dir(src_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "dtb" {
                        if let Some(file_name) = path.file_name() {
                            let dst_path = dst_dir.join(file_name);
                            std::fs::copy(&path, &dst_path).map_err(|e| {
                                format!("Failed to copy DTB {:?} to {:?}: {}", path, dst_path, e)
                            })?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
