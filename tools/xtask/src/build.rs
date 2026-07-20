use std::io;
use std::path::Path;
use std::process::Command;

use crate::config::{Config, Subproject, UserCrate};
use crate::output::run_silent;
use crate::platform::Platform;
use crate::toolchain::{find_objcopy, find_rust_lld};

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

pub fn build(config: &Config, plat: &Platform) -> Result<(), String> {
    println!(
        "{}    Building{} {} Ecosystem ({})",
        BOLD_CYAN, RESET, config.project.name, plat.arch
    );

    let v = get_parsed_version(config);

    // Bootstrap toolchain items
    bootstrap_ohlink_tools(config)?;

    // Iterate over configured subprojects and execute their actions
    for sub in &config.subprojects {
        match sub.subproject_type.as_str() {
            "userspace" => {
                if let Some(crates) = &sub.crates {
                    for u_crate in crates {
                        build_userspace_program(plat, u_crate, &v)?;
                    }
                }
                pack_user_programs(plat, sub)?;
                if let (Some(src_etc), Some(dst_etc)) = (&sub.etc_source, &sub.etc_target) {
                    stage_etc_files(src_etc, dst_etc).map_err(|e| e.to_string())?;
                }
            }
            "kernel" => {
                build_kernel(sub, plat, &v)?;
                link_kernel(sub, plat)?;
                extract_kernel_raw(sub)?;
                pack_kernel_ohc(config, sub, plat)?;
            }
            "bootloader" => {
                build_bootloader(sub, plat, &v)?;
                extract_bootloader_bin(sub, plat)?;
            }
            _ => {
                return Err(format!("Unknown subproject type: {}", sub.subproject_type));
            }
        }
    }

    print_build_summary(config, plat);
    generate_dist_image(config, plat, &v)?;

    println!(
        "\n{}     Success{} {} built successfully!\n",
        BOLD_GREEN, RESET, config.project.name
    );
    Ok(())
}

fn bootstrap_ohlink_tools(config: &Config) -> Result<(), String> {
    for tool in &config.toolchain.bootstrap {
        print!(
            "{}  Bootstrapping{} {} (Host)...",
            BOLD_CYAN, RESET, tool.name
        );
        let mut args = vec!["build"];
        if tool.release {
            args.push("--release");
        }
        args.push("--manifest-path");
        args.push(&tool.path);

        let result = run_silent(Command::new("cargo").args(&args), || {
            println!(
                "\r{}  Bootstrapping{} {} (Host)... Done",
                BOLD_CYAN, RESET, tool.name
            );
        });
        if !result.success {
            return Err(format!("failed to bootstrap {}", tool.name));
        }
    }
    Ok(())
}

fn build_userspace_program(plat: &Platform, u_crate: &UserCrate, v: &ParsedVersion) -> Result<(), String> {
    print!(
        "{}  Building{} {} (EL0)...",
        BOLD_GREEN, RESET, u_crate.crate_name
    );
    let display_str = v.to_display_string();
    let mut cmd = Command::new("cargo");
    cmd.args([
        "+nightly",
        "build",
        "--release",
        "-p",
        &u_crate.crate_name,
        "--target",
        &plat.userspace_target,
        "--no-default-features",
        "--features",
        "capsule",
        "-Z",
        "build-std=core,alloc,panic_abort",
        "-Z",
        "json-target-spec",
    ]);
    cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
    cmd.env("CAPSULEOS_VERSION", &display_str);
    let result = run_silent(&mut cmd, || {
        println!(
            "\r{}  Building{} {} (EL0)... Done",
            BOLD_GREEN, RESET, u_crate.crate_name
        );
    });
    if !result.success {
        Err(format!("failed to build {}", u_crate.crate_name))
    } else {
        Ok(())
    }
}

fn pack_user_programs(plat: &Platform, sub: &Subproject) -> Result<(), String> {
    let staging_bin = sub
        .staging_bin_dir
        .as_ref()
        .ok_or_else(|| "missing staging_bin_dir in userspace config".to_string())?;
    std::fs::create_dir_all(staging_bin).map_err(|e| e.to_string())?;

    let crates = sub
        .crates
        .as_ref()
        .ok_or_else(|| "missing crates in userspace config".to_string())?;

    for u_crate in crates {
        print!("{}  Packing{} {}...", BOLD_GREEN, RESET, u_crate.out_name);
        let elf = format!(
            "build/target/{}-unknown-capsule/release/{}",
            plat.arch,
            u_crate.crate_name.replace("hnx-", "")
        );
        let output = format!("{}/{}", staging_bin, u_crate.out_name);
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
                &u_crate.entry,
            ]),
            || {
                println!(
                    "\r{}  Packing{} {}... Done",
                    BOLD_GREEN, RESET, u_crate.out_name
                );
            },
        );
        if !result.success {
            return Err(format!("failed to pack {}", u_crate.out_name));
        }
    }

    if let Some(rootfs_out) = &sub.rootfs_output {
        print!("{}  Archiving{} rootfs.img...", BOLD_GREEN, RESET);
        if let Some(parent) = Path::new(rootfs_out).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::pack::pack_rootfs("build/dist/staging_rootfs", rootfs_out)
            .map_err(|e| format!("Failed to archive rootfs: {}", e))?;
        println!("\r{}  Archiving{} rootfs.img... Done", BOLD_GREEN, RESET);
    }

    Ok(())
}

fn build_kernel(sub: &Subproject, plat: &Platform, v: &ParsedVersion) -> Result<(), String> {
    let path = sub
        .path
        .as_ref()
        .ok_or_else(|| "missing path in kernel config".to_string())?;
    let package = sub
        .package
        .as_ref()
        .ok_or_else(|| "missing package in kernel config".to_string())?;

    // Clean kernel to prevent caching issues
    let _ = Command::new("cargo")
        .args(["clean"])
        .current_dir(path)
        .output();

    print!("{}  Building{} {} (kernel)...", BOLD_GREEN, RESET, package);
    let display_str = v.to_display_string();
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        &plat.rust_target,
        "-p",
        package,
        "--release",
    ])
    .current_dir(path)
    .env("CAPSULEOS_BUILD_NUM", &v.patch)
    .env("CAPSULEOS_VERSION", &display_str);

    let result = run_silent(&mut cmd, || {
        println!(
            "\r{}  Building{} {} (kernel)... Done",
            BOLD_GREEN, RESET, package
        );
    });
    if !result.success {
        Err(format!("failed to build kernel {}", package))
    } else {
        Ok(())
    }
}

fn link_kernel(sub: &Subproject, plat: &Platform) -> Result<(), String> {
    let link_output = sub
        .link_output
        .as_ref()
        .and_then(|o| o.as_ref())
        .ok_or_else(|| "missing link_output in kernel config".to_string())?;

    if let Some(parent) = Path::new(link_output).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    print!("{}  Linking{} {}...", BOLD_GREEN, RESET, link_output);
    let lld = find_rust_lld();
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
            &format!(
                "kernel/build/target/{}/release/libkernel.a",
                plat.rust_target
            ),
            "--no-whole-archive",
            "-o",
            link_output,
        ]),
        || {
            println!("\r{}  Linking{} {}... Done", BOLD_GREEN, RESET, link_output);
        },
    );
    if !result.success {
        Err("failed to link kernel".to_string())
    } else {
        Ok(())
    }
}

fn extract_kernel_raw(sub: &Subproject) -> Result<(), String> {
    let link_output = sub
        .link_output
        .as_ref()
        .and_then(|o| o.as_ref())
        .ok_or_else(|| "missing link_output in kernel config".to_string())?;
    let raw_output = sub
        .raw_output
        .as_ref()
        .and_then(|o| o.as_ref())
        .ok_or_else(|| "missing raw_output in kernel config".to_string())?;

    print!("{}  Extracting{} {}...", BOLD_GREEN, RESET, raw_output);
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args(["-O", "binary", link_output, raw_output]),
        || {
            println!(
                "\r{}  Extracting{} {}... Done",
                BOLD_GREEN, RESET, raw_output
            );
        },
    );

    if !result.success {
        Err("failed to extract raw binary".to_string())
    } else {
        Ok(())
    }
}

fn pack_kernel_ohc(config: &Config, sub: &Subproject, plat: &Platform) -> Result<(), String> {
    let raw_output = sub
        .raw_output
        .as_ref()
        .and_then(|o| o.as_ref())
        .ok_or_else(|| "missing raw_output in kernel config".to_string())?;
    let ohc_output = sub
        .ohc_output
        .as_ref()
        .and_then(|o| o.as_ref())
        .ok_or_else(|| "missing ohc_output in kernel config".to_string())?;

    print!("{}  Packing{} {}...", BOLD_GREEN, RESET, ohc_output);
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
            &config.toolchain.linker.path,
            "-p",
            &config.toolchain.linker.package,
            "--",
            "--input",
            raw_output,
            "--output",
            ohc_output,
            "--entry",
            &decimal_entry,
        ]),
        || {
            println!("\r{}  Packing{} {}... Done", BOLD_GREEN, RESET, ohc_output);
        },
    );
    if !result.success {
        Err("failed to pack kernel".to_string())
    } else {
        Ok(())
    }
}

fn build_bootloader(sub: &Subproject, plat: &Platform, v: &ParsedVersion) -> Result<(), String> {
    let package = sub
        .package
        .as_ref()
        .ok_or_else(|| "missing package in bootloader config".to_string())?;

    print!("{}  Building{} {}...", BOLD_GREEN, RESET, package);
    let display_str = v.to_display_string();
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--release",
        "-p",
        package,
        "--target",
        &plat.rust_target,
    ]);
    cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
    cmd.env("CAPSULEOS_VERSION", &display_str);

    let result = run_silent(&mut cmd, || {
        println!("\r{}  Building{} {}... Done", BOLD_GREEN, RESET, package);
    });
    if !result.success {
        Err(format!("failed to build bootloader {}", package))
    } else {
        Ok(())
    }
}

fn extract_bootloader_bin(sub: &Subproject, plat: &Platform) -> Result<(), String> {
    let package = sub
        .package
        .as_ref()
        .ok_or_else(|| "missing package in bootloader config".to_string())?;
    let bin_output = sub
        .bin_output
        .as_ref()
        .and_then(|o| o.as_ref())
        .ok_or_else(|| "missing bin_output in bootloader config".to_string())?;

    let resolved_bin = bin_output.replace("{rust_target}", &plat.rust_target);

    if let Some(parent) = Path::new(&resolved_bin).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args([
            "-O",
            "binary",
            &format!("build/target/{}/release/{}", plat.rust_target, package),
            &resolved_bin,
        ]),
        || {},
    );

    if !result.success {
        Err("failed to extract bootloader".to_string())
    } else {
        Ok(())
    }
}

fn print_build_summary(config: &Config, plat: &Platform) {
    let print_size = |label: &str, path: &str| {
        if let Ok(meta) = std::fs::metadata(path) {
            let size_kb = meta.len() as f64 / 1024.0;
            println!("  {}: [{:.1} KB]", label, size_kb);
        }
    };

    // Print size for any kernel/bootloader/userspace outputs we find
    for sub in &config.subprojects {
        match sub.subproject_type.as_str() {
            "kernel" => {
                if let Some(Some(ohc_out)) = &sub.ohc_output {
                    print_size("hnxcore", ohc_out);
                }
            }
            "bootloader" => {
                if let Some(Some(bin_out)) = &sub.bin_output {
                    let resolved_bin = bin_out.replace("{rust_target}", &plat.rust_target);
                    print_size("capsule-bootloader.bin", &resolved_bin);
                }
            }
            "userspace" => {
                if let Some(rootfs_out) = &sub.rootfs_output {
                    print_size("rootfs.img", rootfs_out);
                }
            }
            _ => {}
        }
    }
}

fn generate_dist_image(config: &Config, plat: &Platform, v: &ParsedVersion) -> Result<(), String> {
    let date_output = Command::new("date")
        .arg("+%Y%m%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "20260715".to_string());

    let version_string = format!("{}.{}.{}-{}", v.major, v.minor, v.patch, v.tag);

    let img_name = config
        .distribution
        .image_name_template
        .replace("{project_name}", &v.os_name)
        .replace("{codename}", &v.codename)
        .replace("{version}", &version_string)
        .replace("{arch}", &plat.arch)
        .replace("{date}", &date_output);

    let img_path = format!("{}/{}", config.distribution.output_dir, img_name);

    std::fs::create_dir_all(&config.distribution.output_dir)
        .map_err(|e| format!("Failed to create distribution directory: {}", e))?;

    println!(
        "{}  Packaging{} Release Distribution Image: build/dist/distribution/{}",
        BOLD_CYAN, RESET, img_name
    );

    // Concatenate / Pad configured stages
    let mut final_img_data = Vec::new();

    for stage in &config.distribution.stages {
        let resolved_input = stage.input.replace("{rust_target}", &plat.rust_target);
        let mut data = std::fs::read(&resolved_input).map_err(|e| {
            format!(
                "Failed to read distribution stage: {} ({})",
                resolved_input, e
            )
        })?;

        if let Some(pad_to) = stage.pad_to {
            if data.len() > pad_to as usize {
                return Err(format!(
                    "Stage size ({} bytes) exceeds maximum padding boundary ({} bytes) for {}",
                    data.len(),
                    pad_to,
                    resolved_input
                ));
            }
            data.resize(pad_to as usize, 0);
        }
        final_img_data.extend_from_slice(&data);
    }

    std::fs::write(&img_path, final_img_data)
        .map_err(|e| format!("Failed to write distribution image: {}", e))?;

    if let Ok(meta) = std::fs::metadata(&img_path) {
        let size_kb = meta.len() as f64 / 1024.0;
        println!(
            "  {}Generated{} {} [{:.1} KB]",
            BOLD_GREEN, RESET, img_path, size_kb
        );
    }

    Ok(())
}

fn stage_etc_files(src: &str, dst: &str) -> io::Result<()> {
    let src_path = Path::new(src);
    if !src_path.exists() {
        return Ok(());
    }
    let dst_path = Path::new(dst);
    std::fs::create_dir_all(dst_path)?;
    copy_recursive(src_path, dst_path)?;
    Ok(())
}

fn copy_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_recursive(&child_src, &child_dst)?;
        }
    } else {
        let bytes = std::fs::read(src)?;
        std::fs::write(dst, &bytes)?;
    }
    Ok(())
}

pub struct ParsedVersion {
    pub os_name: String,
    pub codename: String,
    pub major: u32,
    pub minor: u32,
    pub patch: String,
    pub tag: String,
}

impl ParsedVersion {
    pub fn to_display_string(&self) -> String {
        format!("{} {} v{}.{}.{} ({})", self.os_name, self.codename, self.major, self.minor, self.patch, self.tag)
    }
}

pub fn get_parsed_version(config: &Config) -> ParsedVersion {
    let os_name = config.project.name.clone();
    let codename = config.project.codename.clone();

    let mut major = 1;
    let mut minor = 0;

    let raw_ver = &config.project.version;
    let ver_parts: Vec<&str> = raw_ver.split('-').collect();
    let mut tag = if ver_parts.len() >= 2 {
        ver_parts[1].to_string()
    } else {
        "release".to_string()
    };

    let num_parts: Vec<&str> = ver_parts[0].split('.').collect();
    if num_parts.len() >= 1 {
        if let Ok(maj) = num_parts[0].parse::<u32>() {
            major = maj;
        }
    }
    if num_parts.len() >= 2 {
        if let Ok(min) = num_parts[1].parse::<u32>() {
            minor = min;
        }
    }

    let mut patch = "0".to_string();
    let mut git_success = false;

    let match_pattern = format!("{}*", codename);
    if let Ok(output) = Command::new("git")
        .args(["describe", "--tags", "--long", "--match", &match_pattern])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            let trimmed = s.trim();
            let parts: Vec<&str> = trimmed.split('-').collect();
            if parts.len() >= 3 {
                patch = parts[parts.len() - 2].to_string();
                let tag_info = &parts[..parts.len() - 2];
                if tag_info.len() >= 2 {
                    let version_parts: Vec<&str> = tag_info[1].split('.').collect();
                    if version_parts.len() >= 1 {
                        if let Ok(maj) = version_parts[0].parse::<u32>() {
                            major = maj;
                        }
                    }
                    if version_parts.len() >= 2 {
                        if let Ok(min) = version_parts[1].parse::<u32>() {
                            minor = min;
                        }
                    }
                    if tag_info.len() >= 3 {
                        tag = tag_info[2..].join("-");
                    }
                }
                git_success = true;
            }
        }
    }

    if !git_success {
        if let Ok(output) = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                patch = s.trim().to_string();
            }
        }
    }

    ParsedVersion {
        os_name,
        codename,
        major,
        minor,
        patch,
        tag,
    }
}
