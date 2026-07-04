use std::process::Command;

use crate::output::run_silent;
use crate::platform::Platform;

const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

pub fn run(plat: &Platform) -> Result<(), String> {
    println!("{}    Booting{} Launching CapsuleOS in QEMU Emulator...", BOLD_BLUE, RESET);
    generate_qemu_dtb(plat)?;
    launch_qemu(plat);
    Ok(())
}

fn generate_qemu_dtb(plat: &Platform) -> Result<(), String> {
    println!("{}  Generate{} QEMU Device Tree Blob (DTB)...", BOLD_BLUE, RESET);
    let machine = if plat.qemu_arch == "aarch64" { "virt,secure=off" } else { "virt" };
    let mut dump_cmd = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    dump_cmd.args([
        "-M", machine, "-cpu", plat.qemu_cpu, "-m", plat.qemu_mem,
        "-machine", "dumpdtb=/tmp/qemu_raw.dtb", "-display", "none"
    ]);
    for arg in &plat.qemu_extra {
        dump_cmd.arg(arg);
    }
    let result = run_silent(&mut dump_cmd, || {});
    if !result.success { return Err("failed to dump dtb".to_string()); }

    let result = run_silent(
        Command::new("dtc").args(["-I", "dtb", "-O", "dts", "/tmp/qemu_raw.dtb", "-o", "/tmp/qemu.dts"]),
        || {}
    );
    if !result.success { return Err("failed to decompile DTB".to_string()); }

    let result = run_silent(
        Command::new("dtc").args(["-I", "dts", "-O", "dtb", "/tmp/qemu.dts", "-o", "dist/qemu.dtb"]),
        || {}
    );
    if !result.success { return Err("failed to compile DTB".to_string()); }

    let _ = std::fs::remove_file("/tmp/qemu_raw.dtb");
    let _ = std::fs::remove_file("/tmp/qemu.dts");
    Ok(())
}

fn launch_qemu(plat: &Platform) {
    println!("{}  Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}", BOLD_GREEN, RESET, GRAY, RESET);
    let machine = if plat.qemu_arch == "aarch64" { "virt,secure=off" } else { "virt" };
    let mut qemu = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    if plat.qemu_arch == "aarch64" {
        qemu.args([
            "-M", machine, "-cpu", plat.qemu_cpu, "-m", plat.qemu_mem, "-nographic",
            "-device", &format!("loader,file=build/target/{}/release/capsule-bootloader.bin,addr={},cpu-num=0,force-raw=on", plat.rust_target, plat.boot_addr),
            "-device", &format!("loader,file=dist/kernel/hnxcore.ohc,addr={},force-raw=on", plat.ohc_addr),
            "-device", &format!("loader,file=dist/qemu.dtb,addr={},force-raw=on", plat.dtb_addr),
        ]);
    } else {
        qemu.args([
            "-M", machine, "-cpu", plat.qemu_cpu, "-m", plat.qemu_mem, "-nographic",
            "-kernel", &format!("build/target/{}/release/capsule-bootloader", plat.rust_target),
            "-device", &format!("loader,file=dist/kernel/hnxcore.ohc,addr={},force-raw=on", plat.ohc_addr),
            "-device", &format!("loader,file=dist/qemu.dtb,addr={},force-raw=on", plat.dtb_addr),
        ]);
    }
    for arg in &plat.qemu_extra {
        qemu.arg(arg);
    }
    qemu.arg("-semihosting");
    let _ = qemu.status();
}
