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

    generate_qemu_dtb(plat)?;
    launch_qemu(plat, gdb);
    Ok(())
}

fn generate_qemu_dtb(plat: &Platform) -> Result<(), String> {
    println!(
        "{}  Generate{} QEMU Device Tree Blob (DTB)...",
        BOLD_BLUE, RESET
    );

    let mut dump_cmd = Command::new(&plat.qemu_bin);
    // Render dtb_dump_args
    for arg in &plat.qemu_dtb_dump_args {
        let rendered_arg = render_variables(arg, plat);
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

fn launch_qemu(plat: &Platform, gdb: bool) {
    println!(
        "{}  Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}",
        BOLD_GREEN, RESET, GRAY, RESET
    );

    let mut qemu = Command::new(&plat.qemu_bin);
    for arg in &plat.qemu_args {
        let rendered_arg = render_variables(arg, plat);
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

fn render_variables(template: &str, plat: &Platform) -> String {
    template
        .replace("{rust_target}", &plat.rust_target)
        .replace("{boot_addr}", &plat.boot_addr)
        .replace("{ohc_addr}", &plat.ohc_addr)
        .replace("{dtb_addr}", &plat.dtb_addr)
        .replace("{rootfs_addr}", &plat.rootfs_addr)
}
