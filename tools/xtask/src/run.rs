use std::process::Command;

use crate::config::Config;
use crate::output::run_silent;
use crate::platform::Platform;

const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

pub fn run(config: &Config, plat: &Platform, gdb: bool) -> Result<(), String> {
    println!(
        "{}    Booting{} Launching {} in QEMU Emulator...",
        BOLD_BLUE, RESET, config.project.name
    );
    let _ = Command::new("killall").arg(&plat.qemu_bin).status();

    // 1. Resolve dynamic compiled artifact paths from subprojects configuration
    let (boot_bin, boot_bin_raw, kern_bin, r_img) = resolve_artifact_paths(config, plat)?;

    // 2. Generate Device Tree Blob
    generate_qemu_dtb(plat, &boot_bin, &boot_bin_raw, &kern_bin, &r_img)?;

    // 3. Launch QEMU with fully rendered dynamic arguments
    launch_qemu(plat, gdb, &boot_bin, &boot_bin_raw, &kern_bin, &r_img);
    Ok(())
}

pub fn resolve_artifact_paths(
    config: &Config,
    plat: &Platform,
) -> Result<(String, String, String, String), String> {
    let mut bootloader_bin = String::new();
    let mut bootloader_bin_raw = String::new();
    let mut kernel_bin = String::new();
    let mut rootfs_img = String::new();

    for sub in &config.subprojects {
        match sub.subproject_type.as_str() {
            "bootloader" => {
                if let Some(Some(bin_out)) = &sub.bin_output {
                    bootloader_bin = bin_out.replace("{rust_target}", &plat.rust_target);
                    bootloader_bin_raw = bootloader_bin.trim_end_matches(".bin").to_string();
                }
            }
            "kernel" => {
                if let Some(Some(ohc_out)) = &sub.ohc_output {
                    kernel_bin = ohc_out.clone();
                }
            }
            "userspace" => {
                if let Some(rootfs_out) = &sub.rootfs_output {
                    rootfs_img = rootfs_out.clone();
                }
            }
            _ => {}
        }
    }

    if bootloader_bin.is_empty() || kernel_bin.is_empty() || rootfs_img.is_empty() {
        return Err(
            "Failed to resolve compiled artifact paths from subprojects configuration.".to_string(),
        );
    }

    Ok((bootloader_bin, bootloader_bin_raw, kernel_bin, rootfs_img))
}

pub fn generate_qemu_dtb(
    plat: &Platform,
    bootloader_bin: &str,
    bootloader_bin_raw: &str,
    kernel_bin: &str,
    rootfs_img: &str,
) -> Result<(), String> {
    println!(
        "{}  Generate{} QEMU Device Tree Blob (DTB)...",
        BOLD_BLUE, RESET
    );

    let mut dump_cmd = Command::new(&plat.qemu_bin);
    // Render dtb_dump_args dynamically
    for arg in &plat.qemu_dtb_dump_args {
        let rendered_arg = render_variables(
            arg,
            plat,
            bootloader_bin,
            bootloader_bin_raw,
            kernel_bin,
            rootfs_img,
        );
        dump_cmd.arg(rendered_arg);
    }

    let result = run_silent(&mut dump_cmd, || {});
    if !result.success {
        return Err("failed to dump dtb".to_string());
    }

    let result = run_silent(
        Command::new("dtc").args([
            "-I",
            "dtb",
            "-O",
            "dts",
            "/tmp/qemu_raw.dtb",
            "-o",
            "/tmp/qemu.dts",
        ]),
        || {},
    );
    if !result.success {
        return Err("failed to decompile DTB".to_string());
    }

    let result = run_silent(
        Command::new("dtc").args([
            "-I",
            "dts",
            "-O",
            "dtb",
            "/tmp/qemu.dts",
            "-o",
            "build/dist/qemu.dtb",
        ]),
        || {},
    );
    if !result.success {
        return Err("failed to compile DTB".to_string());
    }

    let _ = std::fs::remove_file("/tmp/qemu_raw.dtb");
    let _ = std::fs::remove_file("/tmp/qemu.dts");
    Ok(())
}

fn launch_qemu(
    plat: &Platform,
    gdb: bool,
    bootloader_bin: &str,
    bootloader_bin_raw: &str,
    kernel_bin: &str,
    rootfs_img: &str,
) {
    println!(
        "{}  Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}",
        BOLD_GREEN, RESET, GRAY, RESET
    );

    let mut qemu = Command::new(&plat.qemu_bin);
    for arg in &plat.qemu_args {
        let rendered_arg = render_variables(
            arg,
            plat,
            bootloader_bin,
            bootloader_bin_raw,
            kernel_bin,
            rootfs_img,
        );
        qemu.arg(rendered_arg);
    }

    if gdb {
        qemu.args(["-s", "-S"]);
        println!(
            "{}  GDB Server Enabled{} Listening on TCP port 1234. QEMU CPU suspended. Waiting for GDB...",
            BOLD_BLUE, RESET
        );
    }
    let _ = qemu.status();
}

fn render_variables(
    template: &str,
    plat: &Platform,
    bootloader_bin: &str,
    bootloader_bin_raw: &str,
    kernel_bin: &str,
    rootfs_img: &str,
) -> String {
    template
        .replace("{rust_target}", &plat.rust_target)
        .replace("{boot_addr}", &plat.boot_addr)
        .replace("{ohc_addr}", &plat.ohc_addr)
        .replace("{dtb_addr}", &plat.dtb_addr)
        .replace("{rootfs_addr}", &plat.rootfs_addr)
        .replace("{bootloader_bin}", bootloader_bin)
        .replace("{bootloader_bin_raw}", bootloader_bin_raw)
        .replace("{kernel_bin}", kernel_bin)
        .replace("{rootfs_img}", rootfs_img)
        .replace("{qemu_dtb}", "build/dist/qemu.dtb")
        .replace("{smp}", &plat.qemu_smp.to_string())
}

/// Public entry point used by `xtask code build` to regenerate
/// `build/dist/qemu.dtb` after a clean build, so that downstream
/// tooling that depends on the DTB doesn't have to wait for a
/// separate `xtask code run` step.
pub fn generate_qemu_dtb_artifact_paths(config: &Config, plat: &Platform) -> Result<(), String> {
    let (boot_bin, boot_bin_raw, kern_bin, r_img) = resolve_artifact_paths(config, plat)?;
    // build.rs calls this against the non-"virt" Platform, so we
    // re-resolve a "virt" view to get the actual qemu_* args +
    // SMP value.
    let virt = crate::platform::Platform::from_config(&plat.arch, "virt", config)
        .ok_or_else(|| "virt profile missing for dtb generation".to_string())?;
    generate_qemu_dtb(&virt, &boot_bin, &boot_bin_raw, &kern_bin, &r_img)
}
