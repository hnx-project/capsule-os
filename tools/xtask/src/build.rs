use std::path::Path;
use std::process::Command;
use chrono;

use crate::config::{Resolved, BuildItem};
use crate::output::run_silent;
use crate::platform::Platform;
use crate::toolchain::{find_objcopy, find_rust_lld};

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

pub fn build(resolved: &Resolved, plat: &Platform, generate_dist: bool, test_mode: bool) -> Result<(), String> {
    println!("{}    Building{} capsuleOS Ecosystem (aarch64)", BOLD_CYAN, RESET);

    // 1. Refresh staging directories
    let loader_root = "build/dist/loader_rootfs";
    let staging_root = "build/dist/staging_rootfs";
    let _ = std::fs::remove_dir_all(loader_root);
    let _ = std::fs::remove_dir_all(staging_root);
    std::fs::create_dir_all(loader_root).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(staging_root).map_err(|e| e.to_string())?;

    let v = get_parsed_version(&resolved.root);

    // 2. Build Bootloader
    build_item(&resolved.root.build.bootloader, plat, &v, "bootloader", test_mode)?;
    extract_bootloader_bin(&resolved.root.build.bootloader, plat)?;

    // 3. Build Kernel
    build_item(&resolved.root.build.kernel, plat, &v, "kernel", test_mode)?;
    link_kernel(&resolved.root.build.kernel, plat)?;
    extract_kernel_raw()?;
    pack_kernel_ohc(&resolved.root, &resolved.root.build.kernel, plat)?;

    // 4. Build Libraries
    for lib in &resolved.root.build.libraries {
        build_item(lib, plat, &v, "library", test_mode)?;
    }

    // 5. Build Pillsmod (pills)
    for pill in &resolved.root.build.pillsmod {
        build_item(pill, plat, &v, "pill", test_mode)?;
    }

    // 6. Build Services
    for service in &resolved.root.build.services {
        build_item(service, plat, &v, "service", test_mode)?;
    }

    // 7. Build Apps
    for app in &resolved.root.build.apps {
        build_item(app, plat, &v, "app", test_mode)?;
    }

    // 8. Copy etc files
    stage_etc_files()?;

    // 9. Pack rootfs filesystems
    pack_all_images()?;

    // 10. Print Build summary
    print_sizes(&resolved.root.build, plat);

    if generate_dist {
        generate_dist_image(resolved, plat, &v)?;
    }

    println!("\n{}     Success{} {} built successfully!\n", BOLD_GREEN, RESET, resolved.root.project.name);
    Ok(())
}

fn get_crate_name(cargo_toml_path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .map_err(|e| format!("failed to read {:?}: {}", cargo_toml_path, e))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") {
            if let Some(eq_idx) = trimmed.find('=') {
                let name = trimmed[eq_idx + 1..].trim().trim_matches('"').trim_matches('\'');
                return Ok(name.to_string());
            }
        }
    }
    Err(format!("Could not find package name in {:?}", cargo_toml_path))
}

fn build_item(item: &BuildItem, plat: &Platform, v: &ParsedVersion, category: &str, test_mode: bool) -> Result<(), String> {
    let cargo_toml_path = Path::new(&item.path);
    let package_dir = cargo_toml_path.parent()
        .ok_or_else(|| format!("Invalid path: {}", item.path))?;
    
    let crate_name = get_crate_name(cargo_toml_path)?;

    print!("{}  Building{} {} ({category})...", BOLD_GREEN, RESET, crate_name);
    let display_str = v.to_display_string();

    let mut cmd = Command::new("cargo");
    if category == "kernel" || category == "bootloader" {
        cmd.arg("build").arg("--release");
        cmd.arg("--target").arg(&plat.rust_target);
    } else if category == "library" {
        cmd.arg("+nightly").arg("build").arg("--release");
        cmd.arg("--target").arg(&plat.userspace_target);
        cmd.arg("-Z").arg("build-std=core,alloc,panic_abort");
        cmd.arg("-Z").arg("json-target-spec");
    } else {
        cmd.arg("+nightly").arg("build").arg("--release");
        cmd.arg("--target").arg(&plat.userspace_target);
        cmd.arg("--no-default-features");
        cmd.arg("--features").arg("capsule");
        cmd.arg("-Z").arg("build-std=core,alloc,panic_abort");
        cmd.arg("-Z").arg("json-target-spec");
    }

    cmd.arg("-p").arg(&crate_name);
    cmd.current_dir(package_dir);
    cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
    cmd.env("CAPSULEOS_VERSION", &display_str);

    let result = run_silent(&mut cmd, || {
        println!("\r{}  Building{} {} ({category})... Done", BOLD_GREEN, RESET, crate_name);
    });

    if !result.success {
        return Err(format!("failed to build {category} {}", crate_name));
    }

    // Move user-space programs and pills through ohlink-linker packing
    if category != "kernel" && category != "bootloader" && category != "library" {
        let target_name = Path::new(&plat.userspace_target)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("aarch64-unknown-capsule");

        let elf = format!("build/target/{}/release/{}", target_name, crate_name.replace("hnx-", ""));
        
        let output_bin_path;
        if category == "pill" {
            // Pill is a directory bundle
            let pill_dir = Path::new(&item.output);
            std::fs::create_dir_all(pill_dir).map_err(|e| e.to_string())?;
            output_bin_path = pill_dir.join(crate_name.replace("hnx-", "")).to_string_lossy().to_string();

            // auto.toml copy
            let config_src = package_dir.join("configs/auto.toml");
            if config_src.exists() {
                let config_dst = pill_dir.join("auto.toml");
                let _ = std::fs::copy(&config_src, &config_dst);
            }
        } else {
            // Service / App is a flat executable file
            if let Some(parent) = Path::new(&item.output).parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            output_bin_path = item.output.clone();

            // auto.toml / no.auto.toml configuration copy if present
            let config_src_auto = package_dir.join("configs/auto.toml");
            let config_src_no_auto = package_dir.join("configs/no.auto.toml");
            if config_src_auto.exists() || (test_mode && config_src_no_auto.exists()) {
                let config_src = if test_mode && config_src_no_auto.exists() {
                    config_src_no_auto
                } else {
                    config_src_auto
                };
                if let Some(system_dir) = Path::new(&item.output).parent().and_then(|p| p.parent()) {
                    let config_dst_dir = system_dir.join("share/configs").join(crate_name.replace("hnx-", ""));
                    let _ = std::fs::create_dir_all(&config_dst_dir);
                    let config_dst = config_dst_dir.join("auto.toml");
                    let _ = std::fs::copy(&config_src, &config_dst);
                }
            }
        }

        print!("{}  Packing{} {}...", BOLD_GREEN, RESET, crate_name);

        let result = run_silent(
            Command::new("cargo").args([
                "run",
                "--manifest-path",
                crate::config::TOOLCHAIN_LINKER_PATH,
                "-p",
                crate::config::TOOLCHAIN_LINKER_PACKAGE,
                "--",
                "--input",
                &elf,
                "--output",
                &output_bin_path,
                "--entry",
                "65536",
            ]),
            || {
                println!("\r{}  Packing{} {}... Done", BOLD_GREEN, RESET, crate_name);
            }
        );

        if !result.success {
            return Err(format!("failed to pack userspace program {}", crate_name));
        }
    }

    if category == "library" {
        let target_name = Path::new(&plat.userspace_target)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("aarch64-unknown-capsule");

        let lib_a = format!("build/target/{}/release/lib{}.a", target_name, crate_name);
        
        if let Some(parent) = Path::new(&item.output).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        std::fs::copy(&lib_a, &item.output)
            .map_err(|e| format!("Failed to copy static library {} to {}: {}", lib_a, item.output, e))?;
    }

    Ok(())
}

fn extract_bootloader_bin(item: &BuildItem, plat: &Platform) -> Result<(), String> {
    let elf = format!("build/target/{}/release/capsule-bootloader", plat.rust_target);
    let bin = format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target);

    if let Some(parent) = Path::new(&item.output).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    print!("{}  Extracting bin{} {}...", BOLD_GREEN, RESET, bin);
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args(["-O", "binary", &elf, &bin]),
        || {
            println!("\r{}  Extracting bin{} {}... Done", BOLD_GREEN, RESET, bin);
        }
    );

    if !result.success {
        return Err("failed to extract raw bootloader bin".to_string());
    }

    std::fs::copy(&bin, &item.output).map_err(|e| format!("Failed to copy bootloader to output: {}", e))?;
    Ok(())
}

fn link_kernel(item: &BuildItem, plat: &Platform) -> Result<(), String> {
    let link_output = "build/dist/kernel/kernel.elf";
    if let Some(parent) = Path::new(link_output).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let sub_path = "kernel";
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
                "{}/build/target/{}/release/libkernel.a",
                sub_path,
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

fn extract_kernel_raw() -> Result<(), String> {
    let link_output = "build/dist/kernel/kernel.elf";
    let raw_output = "build/dist/kernel/kernel.raw";
    print!("{}  Extracting raw{} {}...", BOLD_GREEN, RESET, raw_output);
    let objcopy = find_objcopy();
    let result = run_silent(
        Command::new(&objcopy).args(["-O", "binary", link_output, raw_output]),
        || {
            println!("\r{}  Extracting raw{} {}... Done", BOLD_GREEN, RESET, raw_output);
        },
    );
    if !result.success {
        Err("failed to extract raw binary".to_string())
    } else {
        Ok(())
    }
}

fn pack_kernel_ohc(config: &crate::config::RootConfig, item: &BuildItem, plat: &Platform) -> Result<(), String> {
    let raw_output = "build/dist/kernel/kernel.raw";
    let ohc_output = &item.output;

    if let Some(parent) = Path::new(ohc_output).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    print!("{}  Packing{} {}...", BOLD_GREEN, RESET, ohc_output);
    let decimal_entry = plat.kernel_entry.clone();
    let result = run_silent(
        Command::new("cargo").args([
            "run",
            "--manifest-path",
            crate::config::TOOLCHAIN_LINKER_PATH,
            "-p",
            crate::config::TOOLCHAIN_LINKER_PACKAGE,
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

fn stage_etc_files() -> Result<(), String> {
    let src_etc = "kernel/files/etc";
    let dst_etc = "build/dist/staging_rootfs/etc";
    if Path::new(src_etc).exists() {
        let _ = std::fs::create_dir_all(dst_etc);
        copy_dir_recursive(Path::new(src_etc), Path::new(dst_etc))
            .map_err(|e| format!("failed to copy etc config: {}", e))?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(&entry.path(), &dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn pack_all_images() -> Result<(), String> {
    let loader_root = "build/dist/loader_rootfs";
    let staging_root = "build/dist/staging_rootfs";
    let loader_img = "kernel/files/loader.img";
    let rootfs_img = "kernel/files/rootfs.img";

    if let Some(parent) = Path::new(loader_img).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    print!("{}  Archiving{} {}...", BOLD_GREEN, RESET, loader_img);
    crate::pack::pack_rootfs(loader_root, loader_img)
        .map_err(|e| format!("failed to pack loader_img: {}", e))?;
    println!("\r{}  Archiving{} {}... Done", BOLD_GREEN, RESET, loader_img);

    print!("{}  Archiving{} {}...", BOLD_GREEN, RESET, rootfs_img);
    crate::pack::pack_rootfs(staging_root, rootfs_img)
        .map_err(|e| format!("failed to pack rootfs_img: {}", e))?;
    println!("\r{}  Archiving{} {}... Done", BOLD_GREEN, RESET, rootfs_img);

    Ok(())
}

fn print_sizes(build: &crate::config::BuildSection, plat: &Platform) {
    let loader_img = "kernel/files/loader.img";
    let rootfs_img = "kernel/files/rootfs.img";
    let kernel_ohc = &build.kernel.output;
    let bootloader_bin = format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target);

    print_size("loader.img", loader_img);
    print_size("rootfs.img", rootfs_img);
    print_size("hnxcore", kernel_ohc);
    print_size("capsule-bootloader.bin", &bootloader_bin);
}

fn print_size(label: &str, path: &str) {
    if let Ok(meta) = std::fs::metadata(path) {
        let kb = meta.len() as f64 / 1024.0;
        println!("  {}: [{:.1} KB]", label, kb);
    }
}

fn generate_dist_image(resolved: &Resolved, plat: &Platform, v: &ParsedVersion) -> Result<(), String> {
    let config = &resolved.root;
    let output_dir = "build/dist/distribution";
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

    let today = chrono::Local::now().format("%Y%m%d").to_string();
    let image_name = format!(
        "{}-{}-{}-{}-{}.img",
        config.project.name,
        config.project.codename,
        config.project.version,
        plat.arch,
        today
    );
    let output_img = format!("{}/{}", output_dir, image_name);
    
    print!("{}  Generating release distribution image{} {}...", BOLD_GREEN, RESET, output_img);
    let _ = std::fs::write(&output_img, "");
    println!("\r{}  Generated{} {} [0.0 KB]", BOLD_GREEN, RESET, output_img);
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
        format!(
            "{} {} v{}.{}.{} ({})",
            self.os_name, self.codename, self.major, self.minor, self.patch, self.tag
        )
    }
}

pub fn get_parsed_version(config: &crate::config::RootConfig) -> ParsedVersion {
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

    let match_pattern = "v*".to_string();
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
                if tag_info.len() >= 1 {
                    let clean_ver = tag_info[0].trim_start_matches('v');
                    let version_parts: Vec<&str> = clean_ver.split('.').collect();
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
                }
                if tag_info.len() >= 2 {
                    tag = tag_info[1..].join("-");
                } else {
                    tag = "release".to_string();
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
