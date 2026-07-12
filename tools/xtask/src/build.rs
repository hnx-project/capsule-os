use std::process::Command;

use crate::output::run_silent;
use crate::platform::Platform;
use crate::toolchain::{find_objcopy, find_rust_lld};

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

const USERCRATE_S: &[(&str, &str)] = &[
    ("hnx-init", "init"),
    ("hnx-devmgr", "devmgr"),
    ("hnx-fileagent", "fileagent"),
    ("hnx-loader", "loader"),
    ("hnx-osh", "osh"),
    ("hnx-ls", "ls"),
    ("hnx-cat", "cat"),
    ("hnx-mkdir", "mkdir"),
    ("hnx-touch", "touch"),
    ("hnx-rm", "rm"),
    ("hnx-rmdir", "rmdir"),
    ("hnx-ps", "ps"),
    ("hnx-kill", "kill"),
];

pub fn build(plat: &Platform) -> Result<(), String> {
    println!(
        "{}    Building{} CapsuleOS Ecosystem ({})",
        BOLD_CYAN, RESET, plat.arch
    );

    // Bootstrap self-built ohlink-toolchain tools on host first!
    bootstrap_ohlink_tools()?;

    for (crate_name, _out_name) in USERCRATE_S {
        build_userspace_program(plat, crate_name)?;
    }
    pack_user_programs(plat)?;

    build_kernel(plat)?;
    link_kernel(plat)?;
    extract_kernel_raw()?;
    pack_kernel_ohc(plat)?;
    build_bootloader(plat)?;
    extract_bootloader_bin(plat)?;
    print_build_summary(plat);
    generate_dist_image(plat)?;

    println!(
        "\n{}     Success{} CapsuleOS built successfully!\n",
        BOLD_GREEN, RESET
    );
    Ok(())
}

fn bootstrap_ohlink_tools() -> Result<(), String> {
    print!(
        "{}  Bootstrapping{} OHLINK Toolchain (Host)...",
        BOLD_CYAN, RESET
    );
    let result = run_silent(
        Command::new("cargo").args([
            "build",
            "--release",
            "--manifest-path",
            "tools/ohlink-toolchain/Cargo.toml",
        ]),
        || {
            println!(
                "\r{}  Bootstrapping{} OHLINK Toolchain (Host)... Done",
                BOLD_CYAN, RESET
            );
        },
    );
    if !result.success {
        Err("failed to bootstrap OHLINK toolchain".to_string())
    } else {
        Ok(())
    }
}

fn build_userspace_program(plat: &Platform, crate_name: &str) -> Result<(), String> {
    print!("{}  Building{} {} (EL0)...", BOLD_GREEN, RESET, crate_name);
    let userspace_target = format!("std/targets/{}-unknown-capsule.json", plat.arch);
    // Always build the EL0 / "capsule" feature variant.  Some user
    // programs (e.g. `osh`) default to a host stdlib build for local
    // testing; `--no-default-features --features capsule` switches them
    // over to the no_std / hnxlibc-only entry point used by the
    // CapsuleOS user-space ecosystem.  Programs without a `capsule`
    // feature simply ignore the flag.
    let result = run_silent(
        Command::new("cargo").args([
            "+nightly",
            "build",
            "--release",
            "-p",
            crate_name,
            "--target",
            &userspace_target,
            "--no-default-features",
            "--features",
            "capsule",
            "-Z",
            "build-std=core,alloc,panic_abort",
            "-Z",
            "json-target-spec",
        ]),
        || {
            println!(
                "\r{}  Building{} {} (EL0)... Done",
                BOLD_GREEN, RESET, crate_name
            );
        },
    );
    if !result.success {
        Err(format!("failed to build {}", crate_name))
    } else {
        Ok(())
    }
}

fn pack_user_programs(plat: &Platform) -> Result<(), String> {
    let staging_bin = "build/dist/staging_rootfs/system/bin";
    std::fs::create_dir_all(staging_bin).map_err(|e| e.to_string())?;

    for (crate_name, out_name) in USERCRATE_S {
        print!("{}  Packing{} {}...", BOLD_GREEN, RESET, out_name);
        let elf = format!(
            "build/target/{}-unknown-capsule/release/{}",
            plat.arch,
            crate_name.replace("hnx-", "")
        );
        let output = format!("{}/{}", staging_bin, out_name);
        let entry = if *out_name == "init" { "4096" } else { "65536" };
        let result = run_silent(
            Command::new("cargo").args([
                "run",
                "--manifest-path",
                "tools/ohlink-toolchain/Cargo.toml",
                "-p",
                "ohlink-linker",
                "--",
                "--input",
                &elf,
                "--output",
                &output,
                "--entry",
                entry,
            ]),
            || {
                println!("\r{}  Packing{} {}... Done", BOLD_GREEN, RESET, out_name);
            },
        );
        if !result.success {
            return Err(format!("failed to pack {}", out_name));
        }
    }

    // Pack the entire staging_rootfs to a unified rootfs.img inside kernel/files
    print!("{}  Archiving{} rootfs.img...", BOLD_GREEN, RESET);
    std::fs::create_dir_all("kernel/files").map_err(|e| e.to_string())?;
    crate::pack::pack_rootfs("build/dist/staging_rootfs", "kernel/files/rootfs.img")
        .map_err(|e| format!("Failed to archive rootfs: {}", e))?;
    println!("\r{}  Archiving{} rootfs.img... Done", BOLD_GREEN, RESET);

    Ok(())
}

fn build_kernel(plat: &Platform) -> Result<(), String> {
    // Force cargo clean on the kernel crate to prevent incremental cache retaining stale include_bytes! from previous runs.
    let _ = Command::new("cargo")
        .args(["clean"])
        .current_dir("kernel")
        .output();

    print!("{}  Building{} hnx-core (kernel)...", BOLD_GREEN, RESET);
    let result = run_silent(
        Command::new("cargo")
            .args([
                "build",
                "--target",
                plat.rust_target,
                "-p",
                "kernel",
                "--release",
            ])
            .current_dir("kernel"),
        || {
            println!(
                "\r{}  Building{} hnx-core (kernel)... Done",
                BOLD_GREEN, RESET
            );
        },
    );
    if !result.success {
        Err("failed to build kernel".to_string())
    } else {
        Ok(())
    }
}

fn link_kernel(plat: &Platform) -> Result<(), String> {
    std::fs::create_dir_all("build/dist/kernel").map_err(|e| e.to_string())?;
    print!(
        "{}  Linking{} build/dist/kernel/kernel.elf...",
        BOLD_GREEN, RESET
    );
    let lld = find_rust_lld();
    let result = run_silent(
        Command::new(&lld).args([
            "-flavor",
            "gnu",
            "-m",
            plat.ld_emulation,
            "--gc-sections",
            "--whole-archive",
            "-T",
            plat.linker_script,
            &format!(
                "kernel/build/target/{}/release/libkernel.a",
                plat.rust_target
            ),
            "--no-whole-archive",
            "-o",
            "build/dist/kernel/kernel.elf",
        ]),
        || {
            println!(
                "\r{}  Linking{} build/dist/kernel/kernel.elf... Done",
                BOLD_GREEN, RESET
            );
        },
    );
    if !result.success {
        Err("failed to link kernel".to_string())
    } else {
        Ok(())
    }
}

fn extract_kernel_raw() -> Result<(), String> {
    print!(
        "{}  Extracting{} build/dist/kernel/kernel.raw...",
        BOLD_GREEN, RESET
    );
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args([
            "-O",
            "binary",
            "build/dist/kernel/kernel.elf",
            "build/dist/kernel/kernel.raw",
        ]),
        || {
            println!(
                "\r{}  Extracting{} build/dist/kernel/kernel.raw... Done",
                BOLD_GREEN, RESET
            );
        },
    );
    if !result.success {
        Err("failed to extract raw binary".to_string())
    } else {
        Ok(())
    }
}

fn pack_kernel_ohc(plat: &Platform) -> Result<(), String> {
    print!(
        "{}  Packing{} build/dist/kernel/hnxcore...",
        BOLD_GREEN, RESET
    );
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
            "build/dist/kernel/kernel.raw",
            "--output",
            "build/dist/kernel/hnxcore",
            "--entry",
            &decimal_entry,
        ]),
        || {
            println!(
                "\r{}  Packing{} build/dist/kernel/hnxcore... Done",
                BOLD_GREEN, RESET
            );
        },
    );
    if !result.success {
        Err("failed to pack kernel".to_string())
    } else {
        Ok(())
    }
}

fn build_bootloader(plat: &Platform) -> Result<(), String> {
    print!("{}  Building{} capsule-bootloader...", BOLD_GREEN, RESET);
    let result = run_silent(
        Command::new("cargo").args([
            "build",
            "--release",
            "-p",
            "capsule-bootloader",
            "--target",
            plat.rust_target,
        ]),
        || {
            println!(
                "\r{}  Building{} capsule-bootloader... Done",
                BOLD_GREEN, RESET
            );
        },
    );
    if !result.success {
        Err("failed to build bootloader".to_string())
    } else {
        Ok(())
    }
}

fn extract_bootloader_bin(plat: &Platform) -> Result<(), String> {
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args([
            "-O",
            "binary",
            &format!(
                "build/target/{}/release/capsule-bootloader",
                plat.rust_target
            ),
            &format!(
                "build/target/{}/release/capsule-bootloader.bin",
                plat.rust_target
            ),
        ]),
        || {},
    );
    if !result.success {
        Err("failed to extract bootloader".to_string())
    } else {
        Ok(())
    }
}

fn print_build_summary(plat: &Platform) {
    let print_size = |label: &str, path: &str| {
        if let Ok(meta) = std::fs::metadata(path) {
            let size_kb = meta.len() as f64 / 1024.0;
            println!("  {}: [{:.1} KB]", label, size_kb);
        }
    };
    print_size("hnxcore", "build/dist/kernel/hnxcore");
    print_size(
        "capsule-bootloader.bin",
        &format!(
            "build/target/{}/release/capsule-bootloader.bin",
            plat.rust_target
        ),
    );
    print_size("rootfs.img", "kernel/files/rootfs.img");
}

fn generate_dist_image(plat: &Platform) -> Result<(), String> {
    // 1. Get version from Cargo.toml
    let cargo_toml = std::fs::read_to_string("Cargo.toml").map_err(|e| e.to_string())?;
    let version = cargo_toml
        .lines()
        .find(|line| line.starts_with("version ="))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("0.1.0");

    // 2. Get current date in YYYYMMDD format
    let date_output = Command::new("date")
        .arg("+%Y%m%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "20260709".to_string());

    let img_name = format!(
        "capsuleos-pangu-{}-{}-{}.img",
        version, plat.arch, date_output
    );
    let img_path = format!("build/dist/distribution/{}", img_name);

    println!(
        "{}  Packaging{} Release Distribution Image: build/dist/distribution/{}",
        BOLD_CYAN, RESET, img_name
    );

    // 3. Combine bootloader.bin and hnxcore into a single .img file
    let bootloader_path = format!(
        "build/target/{}/release/capsule-bootloader.bin",
        plat.rust_target
    );
    let hnxcore_path = "build/dist/kernel/hnxcore";

    let mut bootloader_data =
        std::fs::read(&bootloader_path).map_err(|e| format!("Failed to read bootloader: {}", e))?;
    let hnxcore_data =
        std::fs::read(hnxcore_path).map_err(|e| format!("Failed to read kernel: {}", e))?;

    // We pad the bootloader to exactly 64KB (65536 bytes) so that the kernel begins at a precise, aligned offset!
    let target_bootloader_size = 65536;
    if bootloader_data.len() > target_bootloader_size {
        return Err(format!(
            "Bootloader size ({} bytes) exceeds maximum padding boundary (64KB)",
            bootloader_data.len()
        ));
    }
    bootloader_data.resize(target_bootloader_size, 0);

    // Combine them
    let mut final_img_data = bootloader_data;
    final_img_data.extend_from_slice(&hnxcore_data);

    // Write distribution image
    std::fs::write(&img_path, final_img_data)
        .map_err(|e| format!("Failed to write distribution image: {}", e))?;

    if let Ok(meta) = std::fs::metadata(&img_path) {
        let size_kb = meta.len() as f64 / 1024.0;
        println!(
            "  {}Generated{} build/dist/distribution/{} [{:.1} KB]",
            BOLD_GREEN, RESET, img_name, size_kb
        );
    }

    Ok(())
}
