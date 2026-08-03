use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

pub struct PartitionSlice<'a> {
    inner: &'a mut std::fs::File,
    start_offset: u64,
    size: u64,
    current_pos: u64,
}

impl<'a> PartitionSlice<'a> {
    pub fn new(inner: &'a mut std::fs::File, start_offset: u64, size: u64) -> io::Result<Self> {
        let mut slice = Self {
            inner,
            start_offset,
            size,
            current_pos: 0,
        };
        slice.seek(SeekFrom::Start(0))?;
        Ok(slice)
    }
}

impl<'a> Read for PartitionSlice<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.current_pos >= self.size {
            return Ok(0);
        }
        let remaining = self.size - self.current_pos;
        let max_read = buf.len().min(remaining as usize);
        self.inner
            .seek(SeekFrom::Start(self.start_offset + self.current_pos))?;
        let bytes_read = self.inner.read(&mut buf[..max_read])?;
        self.current_pos += bytes_read as u64;
        Ok(bytes_read)
    }
}

impl<'a> Write for PartitionSlice<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current_pos >= self.size {
            return Ok(0);
        }
        let remaining = self.size - self.current_pos;
        let max_write = buf.len().min(remaining as usize);
        self.inner
            .seek(SeekFrom::Start(self.start_offset + self.current_pos))?;
        let bytes_written = self.inner.write(&buf[..max_write])?;
        self.current_pos += bytes_written as u64;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> Seek for PartitionSlice<'a> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => self.size as i64 + offset,
            SeekFrom::Current(offset) => self.current_pos as i64 + offset,
        };
        if new_pos < 0 || new_pos > self.size as i64 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Seek out of partition bounds",
            ));
        }
        self.current_pos = new_pos as u64;
        Ok(self.current_pos)
    }
}

type RpiDir<'a, 'b> = fatfs::Dir<
    'b,
    fatfs::StdIoWrapper<PartitionSlice<'a>>,
    fatfs::ChronoTimeProvider,
    fatfs::LossyOemCpConverter,
>;

fn copy_file_to_fat32<'a, 'b>(
    root_dir: &RpiDir<'a, 'b>,
    src_path: &str,
    dst_name: &str,
) -> Result<(), String> {
    let mut dst_file = root_dir
        .create_file(dst_name)
        .map_err(|e| format!("Failed to create file {} in virtual FAT: {:?}", dst_name, e))?;
    let data = std::fs::read(src_path)
        .map_err(|e| format!("Failed to read source file {}: {}", src_path, e))?;
    dst_file
        .write_all(&data)
        .map_err(|e| format!("Failed to write {} data to virtual FAT: {:?}", dst_name, e))?;
    Ok(())
}

fn copy_dir_to_fat32_recursive<'a, 'b>(
    src_dir: &Path,
    parent_dir: &RpiDir<'a, 'b>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(src_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let sub_dir = parent_dir.create_dir(&file_name).map_err(|e| {
                format!("Failed to create dir {} in virtual FAT: {:?}", file_name, e)
            })?;
            copy_dir_to_fat32_recursive(&path, &sub_dir)?;
        } else {
            let mut dst_file = parent_dir.create_file(&file_name).map_err(|e| {
                format!(
                    "Failed to create file {} in virtual FAT: {:?}",
                    file_name, e
                )
            })?;
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            dst_file.write_all(&data).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

use crate::config::{Resolved, Subproject, UserCrate};
use crate::output::run_silent;
use crate::platform::Platform;
use crate::toolchain::{find_objcopy, find_rust_lld};

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

fn generate_c_bindings(config: &crate::config::RootConfig) -> Result<(), String> {
    let cb = match &config.toolchain.c_bindings {
        Some(x) => x,
        None => return Ok(()),
    };
    let bindings = cbindgen::Builder::new()
        .with_crate(&cb.crate_path)
        .with_config(
            cbindgen::Config::from_file(&cb.config_path).unwrap_or_default(),
        )
        .generate()
        .map_err(|e| format!("cbindgen failed: {:?}", e))?;

    if let Some(parent) = Path::new(&cb.output_header).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    bindings.write_to_file(&cb.output_header);
    Ok(())
}

pub fn build(resolved: &Resolved, plat: &Platform, generate_dist: bool) -> Result<(), String> {
    println!(
        "{}    Building{} {} Ecosystem ({})",
        BOLD_CYAN, RESET, resolved.root.project.name, plat.arch
    );

    // Generate C bindings first
    generate_c_bindings(&resolved.root)?;

    let v = get_parsed_version(&resolved.root);

    // Bootstrap toolchain items
    bootstrap_ohlink_tools(&resolved.root)?;

    // Iterate over configured subprojects and execute their actions.
    // `enable = false` skips the subproject without erroring so users
    // can opt out of slow components (e.g. autotools foreign builds)
    // without editing the config file.
    for sub in &resolved.build.subprojects {
        if !sub.enable {
            println!(
                "{}  Skipping{} {} (enable=false)",
                BOLD_CYAN, RESET, sub.name
            );
            continue;
        }
        match sub.subproject_type.as_str() {
            "userspace" => {
                if let Some(crates) = &sub.crates {
                    for u_crate in crates {
                        build_userspace_program(plat, u_crate, &v)?;
                    }
                }
                pack_user_programs(&resolved.root, plat, sub)?;
                if let (Some(src_etc), Some(dst_etc)) = (&sub.etc_source, &sub.etc_target) {
                    stage_etc_files(src_etc, dst_etc).map_err(|e| e.to_string())?;
                }
            }
            "pills" => {
                if let Some(crates) = &sub.crates {
                    for u_crate in crates {
                        build_userspace_program(plat, u_crate, &v)?;
                    }
                }
                pack_pill_bundles(&resolved.root, plat, sub)?;
            }
            "kernel" => {
                build_kernel(sub, plat, &v)?;
                link_kernel(sub, plat)?;
                extract_kernel_raw(sub)?;
                pack_kernel_ohc(&resolved.root, sub, plat)?;
            }
            "bootloader" => {
                build_bootloader(sub, plat, &v)?;
                extract_bootloader_bin(&resolved.root, sub, plat)?;
            }
            "foreign" => {
                // Foreign-build arm: autotools / cmake / gnu-make
                // subprojects run an external configure + build step,
                // then OHLK-pack the resulting ELF.  The implementation
                // lands in a follow-up release; today we just skip so
                // a `enable = true` slot doesn't silently break the
                // build.
                println!(
                    "{}  Foreign{} {} skipped (xtask foreign-build arm not yet implemented)",
                    BOLD_CYAN, RESET, sub.name
                );
            }
            _ => {
                return Err(format!("Unknown subproject type: {}", sub.subproject_type));
            }
        }
    }

    print_build_summary(&resolved.build, plat);

    // Generate the QEMU DTB now (rather than only at `run` time) so
    // that any tool that depends on `build/dist/qemu.dtb` can rely
    // on it being up-to-date after a build.
    if plat.profile == "virt" && plat.qemu_smp > 0 {
        if let Err(e) = crate::run::generate_qemu_dtb_artifact_paths(resolved, plat) {
            eprintln!("warning: QEMU DTB regeneration failed: {}", e);
        }
    }

    if generate_dist {
        generate_dist_image(resolved, plat, &v)?;
    }

    println!(
        "\n{}     Success{} {} built successfully!\n",
        BOLD_GREEN, RESET, resolved.root.project.name
    );
    Ok(())
}

fn bootstrap_ohlink_tools(config: &crate::config::RootConfig) -> Result<(), String> {
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

fn build_userspace_program(
    plat: &Platform,
    u_crate: &UserCrate,
    v: &ParsedVersion,
) -> Result<(), String> {
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

fn pack_user_programs(config: &crate::config::RootConfig, plat: &Platform, sub: &Subproject) -> Result<(), String> {
    let staging_bin = sub
        .staging_bin_dir
        .as_ref()
        .ok_or_else(|| "missing staging_bin_dir in userspace config".to_string())?;
    std::fs::create_dir_all(staging_bin).map_err(|e| e.to_string())?;

    let crates = sub
        .crates
        .as_ref()
        .ok_or_else(|| "missing crates in userspace config".to_string())?;

    let target_name = Path::new(&plat.userspace_target)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("aarch64-unknown-capsule");

    for u_crate in crates {
        print!("{}  Packing{} {}...", BOLD_GREEN, RESET, u_crate.out_name);
        let elf = format!(
            "{}/{}/release/{}",
            config.project.target_dir,
            target_name,
            u_crate.crate_name.replace("hnx-", "")
        );
        let output = format!("{}/{}", staging_bin, u_crate.out_name);
        let result = run_silent(
            Command::new("cargo").args([
                "run",
                "--manifest-path",
                &config.toolchain.linker.path,
                "-p",
                &config.toolchain.linker.package,
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

        if let Some(cfg_path) = &u_crate.config_path {
            let src_path = Path::new(cfg_path);
            if src_path.exists() {
                let staging_bin_path = Path::new(staging_bin);
                let system_dir = staging_bin_path.parent().ok_or_else(|| "staging_bin_dir has no parent".to_string())?;
                let dst_dir = system_dir.join("share/configs").join(&u_crate.out_name);

                if src_path.is_dir() {
                    for entry in std::fs::read_dir(src_path).map_err(|e| e.to_string())? {
                        let entry = entry.map_err(|e| e.to_string())?;
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(ext) = path.extension() {
                                if ext == "toml" {
                                    std::fs::create_dir_all(&dst_dir).map_err(|e| e.to_string())?;
                                    if let Some(file_name) = path.file_name() {
                                        let dst_path = dst_dir.join(file_name);
                                        std::fs::copy(&path, &dst_path).map_err(|e| {
                                            format!("Failed to copy config {:?} to {:?}: {}", path, dst_path, e)
                                        })?;
                                    }
                                }
                            }
                        }
                    }
                } else if src_path.is_file() {
                    std::fs::create_dir_all(&dst_dir).map_err(|e| e.to_string())?;
                    let dst_path = dst_dir.join("auto.toml");
                    std::fs::copy(src_path, &dst_path).map_err(|e| format!("Failed to copy config for {}: {}", u_crate.out_name, e))?;
                }
            }
        }
    }

    if let Some(rootfs_out) = &sub.rootfs_output {
        let staging_root = if let Some(bin_dir) = &sub.staging_bin_dir {
            let p = Path::new(bin_dir);
            p.parent().unwrap().parent().unwrap().to_str().unwrap().to_string()
        } else {
            "build/dist/staging_rootfs".to_string()
        };
        print!("{}  Archiving{} {}...", BOLD_GREEN, RESET, rootfs_out);
        if let Some(parent) = Path::new(rootfs_out).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::pack::pack_rootfs(&staging_root, rootfs_out)
            .map_err(|e| format!("Failed to archive {}: {}", rootfs_out, e))?;
        println!("\r{}  Archiving{} {}... Done", BOLD_GREEN, RESET, rootfs_out);
    }

    Ok(())
}

fn pack_pill_bundles(config: &crate::config::RootConfig, plat: &Platform, sub: &Subproject) -> Result<(), String> {
    let staging_pills = sub
        .staging_bin_dir
        .as_ref()
        .ok_or_else(|| "missing staging_bin_dir in pills config".to_string())?;
    std::fs::create_dir_all(staging_pills).map_err(|e| e.to_string())?;

    let crates = sub
        .crates
        .as_ref()
        .ok_or_else(|| "missing crates in pills config".to_string())?;

    let target_name = Path::new(&plat.userspace_target)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("aarch64-unknown-capsule");

    for u_crate in crates {
        print!("{}  Packing pill bundle{} {}...", BOLD_GREEN, RESET, u_crate.out_name);
        
        let pill_bundle_dir = format!("{}/{}.pill", staging_pills, u_crate.out_name);
        std::fs::create_dir_all(&pill_bundle_dir).map_err(|e| e.to_string())?;

        let elf = format!(
            "{}/{}/release/{}",
            config.project.target_dir,
            target_name,
            u_crate.crate_name.replace("hnx-", "")
        );
        let output = format!("{}/{}", pill_bundle_dir, u_crate.out_name);
        let result = run_silent(
            Command::new("cargo").args([
                "run",
                "--manifest-path",
                &config.toolchain.linker.path,
                "-p",
                &config.toolchain.linker.package,
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
                    "\r{}  Packing pill bundle{} {}... Done",
                    BOLD_GREEN, RESET, u_crate.out_name
                );
            },
        );
        if !result.success {
            return Err(format!("failed to pack {}", u_crate.out_name));
        }

        if let Some(cfg_path) = &u_crate.config_path {
            let src_path = Path::new(cfg_path);
            if src_path.exists() {
                let dst_path = Path::new(&pill_bundle_dir).join("auto.toml");
                if src_path.is_dir() {
                    let toml_src = src_path.join("auto.toml");
                    if toml_src.exists() {
                        std::fs::copy(&toml_src, &dst_path).map_err(|e| {
                            format!("Failed to copy config {:?} to {:?}: {}", toml_src, dst_path, e)
                        })?;
                    }
                } else if src_path.is_file() {
                    std::fs::copy(src_path, &dst_path).map_err(|e| {
                        format!("Failed to copy config {:?} to {:?}: {}", src_path, dst_path, e)
                    })?;
                }
            }
        }
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

    let sub_path = sub
        .path
        .as_ref()
        .ok_or_else(|| "missing path in kernel config".to_string())?;

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

fn pack_kernel_ohc(config: &crate::config::RootConfig, sub: &Subproject, plat: &Platform) -> Result<(), String> {
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

fn extract_bootloader_bin(config: &crate::config::RootConfig, sub: &Subproject, plat: &Platform) -> Result<(), String> {
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
            &format!("{}/{}/release/{}", config.project.target_dir, plat.rust_target, package),
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

fn print_build_summary(build: &crate::config::BuildConfig, plat: &Platform) {
    let print_size = |label: &str, path: &str| {
        if let Ok(meta) = std::fs::metadata(path) {
            let size_kb = meta.len() as f64 / 1024.0;
            println!("  {}: [{:.1} KB]", label, size_kb);
        }
    };

    // Print size for any kernel/bootloader/userspace outputs we find
    for sub in &build.subprojects {
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
                    let label = if rootfs_out.contains("loader.img") {
                        "loader.img"
                    } else {
                        "rootfs.img"
                    };
                    print_size(label, rootfs_out);
                }
            }
            _ => {}
        }
    }
}

fn generate_dist_image(resolved: &Resolved, plat: &Platform, v: &ParsedVersion) -> Result<(), String> {
    let config = &resolved.root;
    if plat.profile == "rpi" {
        // Step 1: Create build output directory and dynamic firmware cache
        std::fs::create_dir_all(&config.distribution.output_dir)
            .map_err(|e| format!("Failed to create distribution directory: {}", e))?;

        let cache_dir = format!("{}/rpi_firmware_cache", config.project.dist_dir());
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache dir: {}", e))?;

        // Download official Broadcom firmware dynamically from stable GitHub URL
        const RPI_FIRMWARE_URLS: &[(&str, &str)] = &[
            (
                "bootcode.bin",
                "https://github.com/raspberrypi/firmware/raw/master/boot/bootcode.bin",
            ),
            (
                "start.elf",
                "https://github.com/raspberrypi/firmware/raw/master/boot/start.elf",
            ),
            (
                "fixup.dat",
                "https://github.com/raspberrypi/firmware/raw/master/boot/fixup.dat",
            ),
            (
                "start4.elf",
                "https://github.com/raspberrypi/firmware/raw/master/boot/start4.elf",
            ),
            (
                "fixup4.dat",
                "https://github.com/raspberrypi/firmware/raw/master/boot/fixup4.dat",
            ),
        ];

        for (name, url) in RPI_FIRMWARE_URLS {
            let cached_path = format!("{}/{}", cache_dir, name);
            if !Path::new(&cached_path).exists() {
                println!(
                    "{}  Downloading{} {} boot firmware dynamically from GitHub...",
                    BOLD_CYAN, RESET, name
                );
                let mut cmd = Command::new("curl");
                cmd.args(["-L", url, "-o", &cached_path]);
                let result = run_silent(&mut cmd, || {});
                if !result.success {
                    return Err(format!(
                        "Failed to download official boot firmware: {}",
                        name
                    ));
                }
            }
        }

        // Build dual-in-one kernel8.img (bootloader padded to 128KB + kernel hnxcore)
        let mut kernel8_data = Vec::new();
        let bootloader_path = format!("{}/aarch64-unknown-none/release/capsule-bootloader.bin", config.project.target_dir);
        let mut boot_data = std::fs::read(&bootloader_path)
            .map_err(|e| format!("Failed to read bootloader from {}: {}", bootloader_path, e))?;
        if boot_data.len() > 131072 {
            return Err(format!(
                "Bootloader size exceeds 128KB: {}",
                boot_data.len()
            ));
        }
        boot_data.resize(131072, 0);
        kernel8_data.extend_from_slice(&boot_data);

        let kernel_path = format!("{}/kernel/hnxcore", config.project.dist_dir());
        let kernel_data =
            std::fs::read(&kernel_path).map_err(|e| format!("Failed to read kernel from {}: {}", kernel_path, e))?;
        kernel8_data.extend_from_slice(&kernel_data);

        // Step 2: Create raw physical SD disk image file
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
            .replace("{arch}", "rpi-aarch64")
            .replace("{date}", &date_output);

        let img_path = format!("{}/{}", config.distribution.output_dir, img_name);

        println!(
            "{}  Formatting{} Raspberry Pi Bootable SD Image: {}",
            BOLD_CYAN, RESET, img_path
        );

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&img_path)
            .map_err(|e| format!("Failed to create physical disk image: {}", e))?;

        let total_sectors = 65536u64; // 32 MB partition image
        file.set_len(total_sectors * 512)
            .map_err(|e| format!("Failed to allocate physical disk size: {}", e))?;

        // Write Master Boot Record (MBR) table at Sector 0
        let mut mbr = [0u8; 512];
        mbr[510] = 0x55;
        mbr[511] = 0xAA;

        let part_offset = 446;
        mbr[part_offset] = 0x80; // Active / Bootable partition
        mbr[part_offset + 1] = 0x00; // CHS start head
        mbr[part_offset + 2] = 0x02; // CHS start sector
        mbr[part_offset + 3] = 0x00; // CHS start cylinder
        mbr[part_offset + 4] = 0x0C; // Partition type: FAT32 with LBA
        mbr[part_offset + 5] = 0xFE; // CHS end head
        mbr[part_offset + 6] = 0x3F; // CHS end sector
        mbr[part_offset + 7] = 0x02; // CHS end cylinder

        let start_lba = 2048u32; // starts at LBA sector 2048 (1MB offset)
        mbr[part_offset + 8..part_offset + 12].copy_from_slice(&start_lba.to_le_bytes());

        let num_sectors = 63488u32; // 65536 - 2048 sectors
        mbr[part_offset + 12..part_offset + 16].copy_from_slice(&num_sectors.to_le_bytes());

        file.write_all(&mbr)
            .map_err(|e| format!("Failed to write MBR block: {}", e))?;

        // Step 3: Write, format and mount the virtual FAT partition slice
        let start_offset = 2048u64 * 512;
        let partition_size = 63488u64 * 512;

        let mut partition_slice = PartitionSlice::new(&mut file, start_offset, partition_size)
            .map_err(|e| format!("Failed to slice MBR partition: {}", e))?;

        let format_opts = fatfs::FormatVolumeOptions::new()
            .volume_label(*b"BOOTFS     ")
            .drive_num(0x80);

        let mut format_wrapper = fatfs::StdIoWrapper::new(&mut partition_slice);
        fatfs::format_volume(&mut format_wrapper, format_opts)
            .map_err(|e| format!("Failed to programmatically format FAT32 partition: {:?}", e))?;

        partition_slice
            .seek(SeekFrom::Start(0))
            .map_err(|e| format!("Failed to seek to partition start: {}", e))?;

        let mount_wrapper = fatfs::StdIoWrapper::new(partition_slice);
        let fs = fatfs::FileSystem::new(mount_wrapper, fatfs::FsOptions::new())
            .map_err(|e| format!("Failed to mount FAT filesystem: {:?}", e))?;

        let root_dir = fs.root_dir();

        // Step 4: Write all standard bootpack files recursively inside the virtual partition
        for (name, _) in RPI_FIRMWARE_URLS {
            let cached_path = format!("{}/{}", cache_dir, name);
            copy_file_to_fat32(&root_dir, &cached_path, name)?;
        }

        // Write config.txt
        let config_txt_content = "\
# CapsuleOS Raspberry Pi Zero 2 W / 3 / 4 config.txt
enable_uart=1
arm_64bit=1
kernel=kernel8.img
kernel_address=0x44000000
initramfs rootfs.img 0x46000000
";
        let mut cfg_file = root_dir
            .create_file("config.txt")
            .map_err(|e| e.to_string())?;
        cfg_file
            .write_all(config_txt_content.as_bytes())
            .map_err(|e| e.to_string())?;

        // Write dual-in-one kernel8.img
        let mut k8_file = root_dir
            .create_file("kernel8.img")
            .map_err(|e| e.to_string())?;
        k8_file
            .write_all(&kernel8_data)
            .map_err(|e| e.to_string())?;

        // Write rootfs.img
        let rootfs_src_owned = resolved.build.subprojects.iter()
            .find(|sub| sub.subproject_type == "userspace")
            .and_then(|sub| sub.rootfs_output.as_ref())
            .cloned()
            .unwrap_or_else(|| "kernel/files/rootfs.img".to_string());
        let rootfs_src = &rootfs_src_owned;
        if Path::new(rootfs_src).exists() {
            let mut rfs_file = root_dir
                .create_file("rootfs.img")
                .map_err(|e| e.to_string())?;
            let rfs_data = std::fs::read(rootfs_src).map_err(|e| e.to_string())?;
            rfs_file.write_all(&rfs_data).map_err(|e| e.to_string())?;
        }

        // Write DTB folder
        let dtb_src_owned = config.distribution.dtb_dir.clone()
            .unwrap_or_else(|| "dtb".to_string());
        let dtb_src = &dtb_src_owned;
        if Path::new(dtb_src).exists() {
            let dtb_dir = root_dir.create_dir("dtb").map_err(|e| e.to_string())?;
            copy_dir_to_fat32_recursive(Path::new(dtb_src), &dtb_dir)?;
        }

        if let Ok(meta) = std::fs::metadata(&img_path) {
            let size_mb = meta.len() as f64 / 1024.0 / 1024.0;
            println!(
                "  {}Generated{} Bootable SD Image: {} [{:.1} MB]",
                BOLD_GREEN, RESET, img_path, size_mb
            );
            println!("  => Dynamic firmware files successfully downloaded & injected.");
            println!("  => Write this single .img file to your SD card using Raspberry Pi Imager or Rufus to boot!");
        }

        return Ok(());
    }

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
