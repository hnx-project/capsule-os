use std::process::Command;
use std::path::Path;
use crate::config::{Resolved, ParsedVersion, resolve_path_placeholders, parse_cargo_toml};
use crate::platform::Platform;
use crate::output::run_silent;
use crate::code::toolchain::{find_objcopy, find_rust_lld};

pub fn build_kernel(
    resolved: &Resolved,
    plat: &Platform,
    v: &ParsedVersion,
    current_step: &mut u32,
    total_steps: u32,
) -> Result<(), String> {
    let kernel = match &resolved.build.kernel {
        Some(k) => k,
        None => return Ok(()),
    };

    use std::io::Write;

    let kernel_dir = Path::new(&kernel.path).parent().ok_or_else(|| {
        "kernel path must point to a valid Cargo.toml inside a directory".to_string()
    })?;

    // Parse the package metadata from the configured Cargo.toml (dynamic!)
    let (crate_name, bin_name) = parse_cargo_toml(&kernel.path)?;

    // Sub-step 1: Compiling
    let comp_step = *current_step;
    *current_step += 1;

    print!("  Compiling ({}/{}) {}...", comp_step, total_steps, crate_name);
    std::io::stdout().flush().unwrap();

    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        &plat.rust_target,
        "--release",
    ])
    .current_dir(kernel_dir);
    
    let display_str = v.to_display_string();
    cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
    cmd.env("CAPSULEOS_VERSION", &display_str);

    let result = run_silent(&mut cmd, || {});
    if !result.success {
        println!(); // ensure newline on error
        return Err(format!("failed to build kernel library {}", crate_name));
    }
    println!("\r  Compiling ({}/{}) {}... DONE\x1B[K", comp_step, total_steps, crate_name);

    // Sub-step 2: Linking
    let link_step = *current_step;
    *current_step += 1;

    let dist_dir = format!("{}/kernel", crate::config::BUILD_DIST);
    std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

    let link_output = format!("{}/kernel.elf", dist_dir);
    
    print!("  Linking ({}/{}) kernel.elf...", link_step, total_steps);
    std::io::stdout().flush().unwrap();

    let lld = find_rust_lld();
    let clean_lib_name = bin_name.replace("-", "_");
    let lib_kernel_path = format!("{}/build/target/{}/release/lib{}.a", kernel_dir.display(), plat.rust_target, clean_lib_name);

    let result = run_silent(
        Command::new(&lld).args([
            "-flavor",
            "gnu",
            "-m",
            &plat.ld_emulation,
            "--gc-sections",
            "--whole-archive",
            "-T",
            &plat.linker_script,
            &lib_kernel_path,
            "--no-whole-archive",
            "-o",
            &link_output,
        ]),
        || {},
    );
    if !result.success {
        println!(); // ensure newline on error
        return Err("failed to link kernel".to_string());
    }
    println!("\r  Linking ({}/{}) kernel.elf... DONE\x1B[K", link_step, total_steps);

    // Sub-step 3: Extracting
    let ext_step = *current_step;
    *current_step += 1;

    let raw_output = format!("{}/kernel.raw", dist_dir);
    
    print!("  Extracting ({}/{}) kernel.raw...", ext_step, total_steps);
    std::io::stdout().flush().unwrap();

    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args(["-O", "binary", &link_output, &raw_output]),
        || {},
    );
    if !result.success {
        println!(); // ensure newline on error
        return Err("failed to extract raw kernel binary".to_string());
    }
    println!("\r  Extracting ({}/{}) kernel.raw... DONE\x1B[K", ext_step, total_steps);

    // Sub-step 4: Packing
    let pack_step = *current_step;
    *current_step += 1;

    let output_resolved = resolve_path_placeholders(&kernel.output);
    let out_name = Path::new(&output_resolved)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("HNXCore");

    print!("  Packing ({}/{}) {}...", pack_step, total_steps, out_name);
    std::io::stdout().flush().unwrap();

    if let Some(parent) = Path::new(&output_resolved).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let decimal_entry = if plat.kernel_entry.starts_with("0x") {
        u64::from_str_radix(plat.kernel_entry.trim_start_matches("0x"), 16)
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "1074266112".to_string())
    } else {
        plat.kernel_entry.to_string()
    };

    let result = run_silent(
        Command::new("cargo").args([
            "run",
            "--manifest-path",
            "tools/ohlink-toolchain/Cargo.toml",
            "-p",
            "ohlink-linker",
            "--",
            "--input",
            &raw_output,
            "--output",
            &output_resolved,
            "--entry",
            &decimal_entry,
        ]),
        || {},
    );

    if !result.success {
        println!(); // ensure newline on error
        Err("failed to pack kernel OHLINK image".to_string())
    } else {
        println!("\r  Packing ({}/{}) {}... DONE\x1B[K", pack_step, total_steps, out_name);
        Ok(())
    }
}
