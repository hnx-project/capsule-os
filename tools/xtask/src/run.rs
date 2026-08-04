use std::process::{Command, Stdio};
use std::fs::File;
use std::io::Write;

use crate::config::Resolved;
use crate::output::run_silent;
use crate::platform::Platform;

const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

pub fn run(resolved: &Resolved, plat: &Platform, gdb: bool) -> Result<(), String> {
    println!(
        "{}    Booting{} Launching {} in QEMU Emulator...",
        BOLD_BLUE, RESET, resolved.root.project.name
    );
    let _ = Command::new("killall").arg(&plat.qemu_bin).status();

    // Resolve dynamic compiled artifact paths.
    let (boot_bin, boot_bin_raw, kern_bin, loader_img, services_img) = resolve_artifact_paths(resolved, plat)?;

    // Generate Device Tree Blob
    let _ = std::fs::create_dir_all(resolved.root.project.dist_dir());

    let disk_img_path = ensure_disk_image(&plat.qemu_disk_img)?;

    generate_qemu_dtb(
        resolved,
        plat,
        &boot_bin,
        &boot_bin_raw,
        &kern_bin,
        &loader_img,
        &services_img,
        &disk_img_path,
    )?;

    // Launch QEMU with fully rendered dynamic arguments
    launch_qemu(
        resolved,
        plat,
        gdb,
        &boot_bin,
        &boot_bin_raw,
        &kern_bin,
        &loader_img,
        &services_img,
        &disk_img_path,
    );
    Ok(())
}

pub fn resolve_artifact_paths(
    _resolved: &Resolved,
    plat: &Platform,
) -> Result<(String, String, String, String, String), String> {
    let bootloader_bin = format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target);
    let bootloader_bin_raw = format!("build/target/{}/release/capsule-bootloader", plat.rust_target);
    let kernel_bin = "build/dist/kernel/hnxcore".to_string();
    let loader_img = "kernel/files/loader.img".to_string();
    let services_img = "kernel/files/rootfs.img".to_string();

    Ok((bootloader_bin, bootloader_bin_raw, kernel_bin, loader_img, services_img))
}

pub fn generate_qemu_dtb(
    resolved: &Resolved,
    plat: &Platform,
    bootloader_bin: &str,
    bootloader_bin_raw: &str,
    kernel_bin: &str,
    loader_img: &str,
    services_img: &str,
    disk_img: &str,
) -> Result<(), String> {
    println!(
        "{}  Generate{} QEMU Device Tree Blob (DTB)...",
        BOLD_BLUE, RESET
    );

    let mut dump_cmd = Command::new(&plat.qemu_bin);
    // Render dtb_dump_args dynamically
    for arg in &plat.qemu_dtb_dump_args {
        let rendered_arg = render_variables(
            resolved,
            arg,
            plat,
            bootloader_bin,
            bootloader_bin_raw,
            kernel_bin,
            loader_img,
            services_img,
            disk_img,
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

    let dtb_output = format!("{}/qemu.dtb", resolved.root.project.dist_dir());
    let result = run_silent(
        Command::new("dtc").args([
            "-I",
            "dts",
            "-O",
            "dtb",
            "/tmp/qemu.dts",
            "-o",
            &dtb_output,
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
    resolved: &Resolved,
    plat: &Platform,
    gdb: bool,
    bootloader_bin: &str,
    bootloader_bin_raw: &str,
    kernel_bin: &str,
    loader_img: &str,
    services_img: &str,
    disk_img: &str,
) {
    println!(
        "{}  Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}",
        BOLD_GREEN, RESET, GRAY, RESET
    );

    let _ = std::fs::create_dir_all(resolved.root.project.dist_dir());

    let mut qemu = Command::new(&plat.qemu_bin);
    for arg in &plat.qemu_args {
        let rendered_arg = render_variables(
            resolved,
            arg,
            plat,
            bootloader_bin,
            bootloader_bin_raw,
            kernel_bin,
            loader_img,
            services_img,
            disk_img,
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

    qemu.stdout(Stdio::inherit());
    qemu.stderr(Stdio::inherit());

    let _ = qemu.status();
}

fn render_variables(
    resolved: &Resolved,
    template: &str,
    plat: &Platform,
    bootloader_bin: &str,
    bootloader_bin_raw: &str,
    kernel_bin: &str,
    loader_img: &str,
    services_img: &str,
    disk_img: &str,
) -> String {
    let dtb_output = format!("{}/qemu.dtb", resolved.root.project.dist_dir());
    template
        .replace("{rust_target}", &plat.rust_target)
        .replace("{boot_addr}", &plat.boot_addr)
        .replace("{ohc_addr}", &plat.ohc_addr)
        .replace("{dtb_addr}", &plat.dtb_addr)
        .replace("{rootfs_addr}", &plat.rootfs_addr)
        .replace("{services_addr}", "0x48000000")
        .replace("{bootloader_bin}", bootloader_bin)
        .replace("{bootloader_bin_raw}", bootloader_bin_raw)
        .replace("{kernel_bin}", kernel_bin)
        .replace("{loader_img}", loader_img)
        .replace("{rootfs_img}", services_img)
        .replace("{qemu_dtb}", &dtb_output)
        .replace("{disk_img}", disk_img)
        .replace("{smp}", &plat.qemu_smp.to_string())
}

pub fn ensure_disk_image(path: &str) -> Result<String, String> {
    let p = std::path::Path::new(path);
    if p.exists() {
        return std::fs::canonicalize(p)
            .map(|c| c.to_string_lossy().into_owned())
            .map_err(|e| format!("failed to canonicalize {}: {}", path, e));
    }

    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let sector_size: usize = 512;
    let total_sectors: usize = 2880;
    let image_size = sector_size * total_sectors;
    let mut image = vec![0u8; image_size];

    // Boot sector signature (FAT12).
    image[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    image[3..11].copy_from_slice(b"MSDOS5.0");
    image[11..13].copy_from_slice(&(sector_size as u16).to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1u16.to_le_bytes());
    image[16] = 2;
    image[17..19].copy_from_slice(&224u16.to_le_bytes());
    image[19..21].copy_from_slice(&(total_sectors as u16).to_le_bytes());
    image[21] = 0xF8;
    image[22..24].copy_from_slice(&9u16.to_le_bytes());
    image[24..26].copy_from_slice(&18u16.to_le_bytes());
    image[26..28].copy_from_slice(&2u16.to_le_bytes());
    image[28..32].copy_from_slice(&0u32.to_le_bytes());
    image[32..36].copy_from_slice(&0u32.to_le_bytes());
    image[36] = 0;
    image[37] = 0;
    image[38] = 0x29;
    image[39..43].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    image[43..54].copy_from_slice(b"BOOTFS     ");
    image[54..62].copy_from_slice(b"FAT12   ");
    image[510..512].copy_from_slice(&[0x55, 0xAA]);

    // FAT12 requires first two entries to be F8 FF FF.
    image[512..515].copy_from_slice(&[0xF8, 0xFF, 0xFF]);
    image[5120..5123].copy_from_slice(&[0xF8, 0xFF, 0xFF]);

    let mut f = File::create(p)
        .map_err(|e| format!("failed to create {}: {}", p.display(), e))?;
    f.write_all(&image)
        .map_err(|e| format!("failed to write {}: {}", p.display(), e))?;

    std::fs::canonicalize(p)
        .map(|c| c.to_string_lossy().into_owned())
        .map_err(|e| format!("failed to canonicalize {}: {}", path, e))
}

pub fn generate_qemu_dtb_artifact_paths(resolved: &Resolved, plat: &Platform) -> Result<(), String> {
    let (boot_bin, boot_bin_raw, kern_bin, loader_img, services_img) = resolve_artifact_paths(resolved, plat)?;
    let _ = std::fs::create_dir_all(resolved.root.project.dist_dir());
    let disk_img = ensure_disk_image(&plat.qemu_disk_img)?;
    generate_qemu_dtb(
        resolved,
        plat,
        &boot_bin,
        &boot_bin_raw,
        &kern_bin,
        &loader_img,
        &services_img,
        &disk_img,
    )
}
