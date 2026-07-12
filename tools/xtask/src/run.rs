use std::process::Command;

use crate::output::run_silent;
use crate::platform::Platform;

const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_GREEN: &str = "\x1b[1;32m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

pub fn run(plat: &Platform, gdb: bool) -> Result<(), String> {
    println!(
        "{}    Booting{} Launching CapsuleOS in QEMU Emulator...",
        BOLD_BLUE, RESET
    );
    let _ = Command::new("killall")
        .arg(format!("qemu-system-{}", plat.qemu_arch))
        .status();
    generate_qemu_dtb(plat)?;
    launch_qemu(plat, gdb);
    Ok(())
}

fn generate_qemu_dtb(plat: &Platform) -> Result<(), String> {
    println!(
        "{}  Generate{} QEMU Device Tree Blob (DTB)...",
        BOLD_BLUE, RESET
    );
    let machine = if plat.qemu_arch == "aarch64" {
        "virt,secure=off"
    } else {
        "virt"
    };
    let mut dump_cmd = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    dump_cmd.args([
        "-M",
        machine,
        "-cpu",
        plat.qemu_cpu,
        "-m",
        plat.qemu_mem,
        "-machine",
        "dumpdtb=/tmp/qemu_raw.dtb",
        "-display",
        "none",
    ]);
    for arg in &plat.qemu_extra {
        dump_cmd.arg(arg);
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

fn launch_qemu(plat: &Platform, gdb: bool) {
    println!(
        "{}  Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}",
        BOLD_GREEN, RESET, GRAY, RESET
    );
    let machine = if plat.qemu_arch == "aarch64" {
        "virt,secure=off"
    } else {
        "virt"
    };
    let mut qemu = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    if plat.qemu_arch == "aarch64" {
        qemu.args([
            "-M", machine, "-cpu", plat.qemu_cpu, "-m", plat.qemu_mem, "-nographic",
            "-device", &format!("loader,file=build/target/{}/release/capsule-bootloader.bin,addr={},cpu-num=0,force-raw=on", plat.rust_target, plat.boot_addr),
            "-device", &format!("loader,file=build/dist/kernel/hnxcore,addr={},force-raw=on", plat.ohc_addr),
            "-device", &format!("loader,file=build/dist/qemu.dtb,addr={},force-raw=on", plat.dtb_addr),
        ]);
    } else {
        qemu.args([
            "-M",
            machine,
            "-cpu",
            plat.qemu_cpu,
            "-m",
            plat.qemu_mem,
            "-nographic",
            "-kernel",
            &format!(
                "build/target/{}/release/capsule-bootloader",
                plat.rust_target
            ),
            "-device",
            &format!(
                "loader,file=build/dist/kernel/hnxcore,addr={},force-raw=on",
                plat.ohc_addr
            ),
            "-device",
            &format!(
                "loader,file=build/dist/qemu.dtb,addr={},force-raw=on",
                plat.dtb_addr
            ),
        ]);
    }
    for arg in &plat.qemu_extra {
        qemu.arg(arg);
    }
    qemu.arg("-semihosting");
    if gdb {
        qemu.args(["-s", "-S"]);
        println!(
            "{}  GDB Server Enabled{} Listening on TCP port 1234. QEMU CPU suspended. Waiting for GDB...",
            BOLD_BLUE, RESET
        );
    }
    let _ = qemu.status();
}

#[allow(dead_code)]
fn get_latest_dist_image(arch: &str) -> Result<String, String> {
    let cargo_toml = std::fs::read_to_string("Cargo.toml").map_err(|e| e.to_string())?;
    let version = cargo_toml
        .lines()
        .find(|line| line.starts_with("version ="))
        .and_then(|line| line.split('"').nth(1))
        .unwrap_or("0.1.0");

    let date_output = Command::new("date")
        .arg("+%Y%m%d")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "20260709".to_string());

    let img_name = format!("capsuleos-pangu-{}-{}-{}.img", version, arch, date_output);
    let img_path = format!("build/dist/distribution/{}", img_name);
    if std::path::Path::new(&img_path).exists() {
        Ok(img_path)
    } else {
        Err(format!("Distribution image not found at {}", img_path))
    }
}
