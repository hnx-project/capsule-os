use std::process::{Command, Stdio};
use crate::config::{Resolved, BUILD_TEMP_RESOURCE, BUILD_TEMP_EFI, BUILD_TEMP_ROOTFS};
use crate::platform::Platform;

const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

pub fn run(resolved: &Resolved, plat: &Platform, gdb: bool) -> Result<(), String> {
    println!(
        "{}    Booting{} Launching {} in UEFI QEMU Emulator...",
        BOLD_BLUE, RESET, resolved.root.project.name
    );
    let _ = Command::new("killall").arg(&plat.qemu_bin).status();

    // 1. Automatically detect standard host AArch64 UEFI BIOS firmware path
    let uefi_bios_path = find_uefi_bios_path()?;
    println!("  Using UEFI Firmware: {}", uefi_bios_path);

    // 1.5 Automatically dump/generate QEMU's dynamic DTB matching the current config dynamically.
    //     The dumped `qemu_virt.dtb` lands on the primary U-Disk (BUILD_TEMP_RESOURCE) so the
    //     UEFI bootloader can load `\boot\qemu_virt.dtb` and hand the kernel a valid DTB even
    //     when the UEFI firmware does not publish an FDT configuration table.
    if plat.arch == "aarch64" && (plat.qemu_bin.contains("qemu-system-aarch64") || plat.qemu_bin.contains("qemu")) {
        std::fs::create_dir_all(format!("{}/boot", BUILD_TEMP_RESOURCE)).unwrap();
        let dtb_output_path = format!("{}/boot/qemu_virt.dtb", BUILD_TEMP_RESOURCE);
        let mut dtb_cmd = Command::new(&plat.qemu_bin);
        let dtb_args_rendered = render_qemu_args(&plat.qemu_args, &uefi_bios_path, plat.qemu_smp, Some(&dtb_output_path));
        dtb_cmd.args(&dtb_args_rendered);
        let _ = dtb_cmd.status();

        // Mirror onto the legacy temp_rootfs dir for dual-drive configs.
        if BUILD_TEMP_ROOTFS != BUILD_TEMP_RESOURCE {
            std::fs::create_dir_all(format!("{}/boot", BUILD_TEMP_ROOTFS)).unwrap();
            let _ = std::fs::copy(&dtb_output_path, format!("{}/boot/qemu_virt.dtb", BUILD_TEMP_ROOTFS));
        }
    }

    // 2. Launch QEMU by mounting build/dist/temp_resource as virtual U-Disk
    launch_qemu(resolved, plat, gdb, &uefi_bios_path);
    Ok(())
}

/// Render `[run.qemu] args`, templating the runtime placeholders and
/// (optionally) appending `dumpdtb=<path>` to the `-M` machine argument
/// so the DTB extractor pass and the real launch share one code path.
fn render_qemu_args(raw_args: &[String], uefi_bios: &str, smp: u32, dump_dtb: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = raw_args.iter();
    while let Some(arg) = iter.next() {
        let rendered = arg
            .replace("{uefi_bios}", uefi_bios)
            .replace("{disk_dir}", BUILD_TEMP_RESOURCE)
            .replace("{efi_dir}", BUILD_TEMP_EFI)
            .replace("{rootfs_dir}", BUILD_TEMP_ROOTFS)
            .replace("{smp}", &smp.to_string());

        if rendered == "-M" {
            out.push(rendered);
            if let Some(next_arg) = iter.next() {
                let m_arg = next_arg
                    .replace("{uefi_bios}", uefi_bios)
                    .replace("{disk_dir}", BUILD_TEMP_RESOURCE)
                    .replace("{efi_dir}", BUILD_TEMP_EFI)
                    .replace("{rootfs_dir}", BUILD_TEMP_ROOTFS)
                    .replace("{smp}", &smp.to_string());
                match dump_dtb {
                    Some(path) => out.push(format!("{},dumpdtb={}", m_arg, path)),
                    None => out.push(m_arg),
                }
            }
        } else {
            out.push(rendered);
        }
    }
    out
}

pub fn find_uefi_bios_path() -> Result<String, String> {
    let standard_paths = [
        "/opt/homebrew/share/qemu/edk2-aarch64-code.fd",      // macOS (Apple Silicon)
        "/usr/local/share/qemu/edk2-aarch64-code.fd",        // macOS (Intel Mac)
        "/usr/share/AAVMF/AAVMF_CODE.fd",                     // Linux (Ubuntu/Debian)
        "/usr/share/qemu-efi-aarch64/QEMU_EFI.fd",            // Linux Standard Path
    ];
    for path in &standard_paths {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    Err("Could not find standard AArch64 UEFI BIOS firmware (edk2-aarch64-code.fd) on your system. Please verify QEMU is installed.".to_string())
}

fn launch_qemu(_resolved: &Resolved, plat: &Platform, gdb: bool, uefi_bios: &str) {
    println!(
        "{}  Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}",
        BOLD_GREEN, RESET, GRAY, RESET
    );

    let mut qemu = Command::new(&plat.qemu_bin);
    let rendered_args = render_qemu_args(&plat.qemu_args, uefi_bios, plat.qemu_smp, None);
    qemu.args(&rendered_args);

    if gdb {
        qemu.args(["-s", "-S"]);
        println!(
            "{}  GDB Server Enabled{} Listening on TCP port 1234. Waiting for connection...",
            BOLD_BLUE, RESET
        );
    }

    qemu.stdout(Stdio::inherit());
    qemu.stderr(Stdio::inherit());

    let _ = qemu.status();
}
