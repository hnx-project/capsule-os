use std::path::{Path, PathBuf};
use std::process::Command;

const BOLD_GREEN: &str = "\x1b[1;32m";
const BOLD_BLUE: &str = "\x1b[1;34m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

struct Platform {
    arch: &'static str,
    rust_target: &'static str,
    kernel_entry: &'static str,
    ld_emulation: &'static str,
    linker_script: &'static str,
    qemu_arch: &'static str,
    qemu_cpu: &'static str,
    qemu_mem: &'static str,
    qemu_extra: Vec<&'static str>,
    dtb_addr: &'static str,
    ohc_addr: &'static str,
    boot_addr: &'static str,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }

    let cmd = &args[1];
    let mut arch = "aarch64";

    // Parse options
    for i in 2..args.len() {
        if args[i] == "--arch" && i + 1 < args.len() {
            arch = &args[i + 1];
        }
    }

    let plat = if arch == "aarch64" {
        Platform {
            arch: "aarch64",
            rust_target: "aarch64-unknown-none",
            kernel_entry: "1074266112", // 0x40080000
            ld_emulation: "aarch64elf",
            linker_script: "kernel/linker/kernel_aarch64.ld",
            qemu_arch: "aarch64",
            qemu_cpu: "cortex-a72",
            qemu_mem: "512M",
            qemu_extra: vec![],
            dtb_addr: "0x42000000",
            ohc_addr: "0x40700000",
            boot_addr: "0x44000000",
        }
    } else if arch == "riscv64" {
        Platform {
            arch: "riscv64",
            rust_target: "riscv64imac-unknown-none-elf",
            kernel_entry: "2148007936", // 0x80080000
            ld_emulation: "elf64lriscv",
            linker_script: "kernel/linker/kernel_riscv64.ld",
            qemu_arch: "riscv64",
            qemu_cpu: "rv64",
            qemu_mem: "512M",
            qemu_extra: vec!["-bios", "default"],
            dtb_addr: "0x82000000",
            ohc_addr: "0x80700000",
            boot_addr: "0x44000000",
        }
    } else {
        panic!("Unsupported architecture: {}", arch);
    };

    match cmd.as_str() {
        "build" => {
            build(&plat);
        }
        "run" => {
            build(&plat);
            run(&plat);
        }
        _ => {
            print_help();
        }
    }
}

fn print_help() {
    println!("CapsuleOS xtask Build System");
    println!("Usage:");
    println!("  cargo xtask build [--arch <aarch64|riscv64>]");
    println!("  cargo xtask run   [--arch <aarch64|riscv64>]");
}

fn find_rust_lld() -> PathBuf {
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("failed to run rustc");
    let sysroot = std::str::from_utf8(&output.stdout).unwrap().trim();
    let rustlib = Path::new(sysroot).join("lib").join("rustlib");
    if let Ok(dirs) = std::fs::read_dir(rustlib) {
        for entry in dirs {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_dir() {
                    let lld = path.join("bin").join("rust-lld");
                    if lld.exists() {
                        return lld;
                    }
                }
            }
        }
    }
    PathBuf::from("rust-lld")
}

fn find_objcopy() -> PathBuf {
    // 1. Homebrew LLVM fallback on Apple Silicon macOS
    let hb_objcopy = PathBuf::from("/opt/homebrew/opt/llvm/bin/llvm-objcopy");
    if hb_objcopy.exists() {
        return hb_objcopy;
    }
    // 2. Search inside rustup sysroot
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("failed to run rustc");
    let sysroot = std::str::from_utf8(&output.stdout).unwrap().trim();
    let rustlib = Path::new(sysroot).join("lib").join("rustlib");
    if let Ok(dirs) = std::fs::read_dir(rustlib) {
        for entry in dirs {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_dir() {
                    let objcopy = path.join("bin").join("llvm-objcopy");
                    if objcopy.exists() {
                        return objcopy;
                    }
                }
            }
        }
    }
    PathBuf::from("llvm-objcopy")
}

fn build(plat: &Platform) {
    println!("{}    Building{} CapsuleOS Ecosystem ({})", BOLD_CYAN, RESET, plat.arch);

    // 1. Compile HNX Core Kernel Submodule
    println!("{}     Compile{} hnx-core (kernel) in release mode...", BOLD_GREEN, RESET);
    let status = Command::new("cargo")
        .args(["build", "--target", plat.rust_target, "-p", "kernel", "--release"])
        .current_dir("kernel")
        .status()
        .expect("failed to execute cargo build on kernel");
    if !status.success() {
        panic!("failed to build kernel");
    }

    // 2. Prepare dist/kernel directory
    std::fs::create_dir_all("dist/kernel").unwrap();

    // 3. Link Kernel ELF using rust-lld
    println!("{}        Link{} dist/kernel/kernel.elf...", BOLD_GREEN, RESET);
    let lld = find_rust_lld();
    let status = Command::new(lld)
        .args([
            "-flavor", "gnu",
            "-m", plat.ld_emulation,
            "--gc-sections",
            "--whole-archive",
            "-T", plat.linker_script,
            &format!("kernel/build/target/{}/release/libkernel.a", plat.rust_target),
            "--no-whole-archive",
            "-o", "dist/kernel/kernel.elf"
        ])
        .status()
        .expect("failed to execute linker");
    if !status.success() {
        panic!("failed to link kernel");
    }

    // 4. Convert ELF to Raw Binary using llvm-objcopy
    println!("{}     Extract{} dist/kernel/kernel.raw...", BOLD_GREEN, RESET);
    let objcopy = find_objcopy();
    let status = Command::new(&objcopy)
        .args(["-O", "binary", "dist/kernel/kernel.elf", "dist/kernel/kernel.raw"])
        .status()
        .expect("failed to execute objcopy");
    if !status.success() {
        panic!("failed to extract raw binary");
    }

    // 5. Pack Kernel to OHC format using ohc-tool
    println!("{}     Package{} dist/kernel/hnxcore.ohc...", BOLD_GREEN, RESET);
    let status = Command::new("cargo")
        .args([
            "run",
            "--manifest-path", "kernel/Cargo.toml",
            "-p", "ohc-tool",
            "--",
            "pack",
            "--input", "dist/kernel/kernel.raw",
            "--output", "dist/kernel/hnxcore.ohc",
            "--entry", plat.kernel_entry
        ])
        .status()
        .expect("failed to run ohc-tool");
    if !status.success() {
        panic!("failed to pack OHC image");
    }

    // 6. Build capsule-bootloader
    println!("{}     Compile{} capsule-bootloader in release mode...", BOLD_GREEN, RESET);
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "capsule-bootloader", "--target", plat.rust_target])
        .status()
        .expect("failed to compile bootloader");
    if !status.success() {
        panic!("failed to build bootloader");
    }

    // 7. Objcopy bootloader to .bin raw image
    let status = Command::new(&objcopy)
        .args([
            "-O", "binary",
            &format!("build/target/{}/release/capsule-bootloader", plat.rust_target),
            &format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target),
        ])
        .status()
        .expect("failed to execute objcopy on bootloader");
    if !status.success() {
        panic!("failed to extract raw bootloader");
    }

    // Report sizes dynamically with safe border alignments
    println!("\n{}+─────────────────────────────────────────────────────────────+{}", GRAY, RESET);
    let print_line = |label: &str, file: &str, size_kb: f64| {
        let content = format!("{}  {} [{:.1} KB]", label, file, size_kb);
        let padded_content = format!("{:<57}", content);
        println!("{}|{} {}{} {}|{}", GRAY, BOLD_GREEN, padded_content, RESET, GRAY, RESET);
    };

    if let Ok(meta) = std::fs::metadata("dist/kernel/hnxcore.ohc") {
        let size_kb = meta.len() as f64 / 1024.0;
        print_line("     Created", "dist/kernel/hnxcore.ohc", size_kb);
    }
    if let Ok(meta) = std::fs::metadata(format!("build/target/{}/release/capsule-bootloader.bin", plat.rust_target)) {
        let size_kb = meta.len() as f64 / 1024.0;
        print_line("     Created", "capsule-bootloader.bin", size_kb);
    }
    println!("{}+─────────────────────────────────────────────────────────────+{}", GRAY, RESET);

    println!("\n{}     Success{} CapsuleOS built successfully! ✨\n", BOLD_GREEN, RESET);
}

fn run(plat: &Platform) {
    println!("{}     Booting{} Launching CapsuleOS in QEMU Emulator...", BOLD_CYAN, RESET);

    // 1. Generate QEMU DTB dynamically
    println!("{}    Generate{} QEMU Device Tree Blob (DTB)...", BOLD_BLUE, RESET);
    let mut dump_cmd = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    dump_cmd.args([
        "-M", "virt,secure=off",
        "-cpu", plat.qemu_cpu,
        "-m", plat.qemu_mem,
        "-machine", "dumpdtb=/tmp/qemu_raw.dtb",
        "-display", "none"
    ]);
    for arg in &plat.qemu_extra {
        dump_cmd.arg(arg);
    }
    let status = dump_cmd.status().expect("failed to dump dtb");
    if !status.success() {
        panic!("failed to dump DTB");
    }

    let status = Command::new("dtc")
        .args(["-I", "dtb", "-O", "dts", "/tmp/qemu_raw.dtb", "-o", "/tmp/qemu.dts"])
        .status()
        .expect("failed to run dtc decompile");
    if !status.success() {
        panic!("failed to decompile DTB");
    }

    let status = Command::new("dtc")
        .args(["-I", "dts", "-O", "dtb", "/tmp/qemu.dts", "-o", "dist/qemu.dtb"])
        .status()
        .expect("failed to run dtc compile");
    if !status.success() {
        panic!("failed to compile DTB");
    }

    let _ = std::fs::remove_file("/tmp/qemu_raw.dtb");
    let _ = std::fs::remove_file("/tmp/qemu.dts");

    // 2. Launch QEMU
    println!("{}     Running{} QEMU virtual machine. {}[Ctrl+A, X to exit]{}", BOLD_GREEN, RESET, GRAY, RESET);
    let mut qemu = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    qemu.args([
        "-M", "virt,secure=off",
        "-cpu", plat.qemu_cpu,
        "-m", plat.qemu_mem,
        "-nographic",
        "-device", &format!("loader,file=build/target/{}/release/capsule-bootloader.bin,addr={},cpu-num=0,force-raw=on", plat.rust_target, plat.boot_addr),
        "-device", &format!("loader,file=dist/kernel/hnxcore.ohc,addr={},force-raw=on", plat.ohc_addr),
        "-device", &format!("loader,file=dist/qemu.dtb,addr={},force-raw=on", plat.dtb_addr),
    ]);
    for arg in &plat.qemu_extra {
        qemu.arg(arg);
    }
    qemu.arg("-semihosting");

    let status = qemu.status().expect("failed to run QEMU");
    if !status.success() {
        panic!("QEMU exited with error");
    }
}
