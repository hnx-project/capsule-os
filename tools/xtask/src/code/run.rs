use std::process::{Command, Stdio};
use crate::config::{Resolved, BUILD_TEMP_RESOURCE};
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

    // 2. Launch QEMU by mounting build/dist/temp_resource as virtual U-Disk
    launch_qemu(resolved, plat, gdb, &uefi_bios_path);
    Ok(())
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
    for arg in &plat.qemu_args {
        let rendered_arg = arg
            .replace("{uefi_bios}", uefi_bios)
            .replace("{disk_dir}", BUILD_TEMP_RESOURCE)
            .replace("{smp}", &plat.qemu_smp.to_string());
        qemu.arg(rendered_arg);
    }

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
