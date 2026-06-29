use std::path::{Path, PathBuf};
use std::process::Command;

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
    println!("[Xtask] Building CapsuleOS Ecosystem ({})", plat.arch);

    // 1. Compile HNX Core Kernel Submodule
    println!("[Xtask] 1. Compiling hnx-core (kernel)...");
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
    println!("[Xtask] 2. Linking kernel.elf...");
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
    println!("[Xtask] 3. Extracting raw kernel.raw...");
    let objcopy = find_objcopy();
    let status = Command::new(&objcopy)
        .args(["-O", "binary", "dist/kernel/kernel.elf", "dist/kernel/kernel.raw"])
        .status()
        .expect("failed to execute objcopy");
    if !status.success() {
        panic!("failed to extract raw binary");
    }

    // 5. Pack Kernel to OHC format using ohc-tool
    println!("[Xtask] 4. Packaging kernel to hnxcore.ohc...");
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
    println!("[Xtask] 5. Compiling capsule-bootloader...");
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

    println!("[Xtask] Build Succeeded!");
}

fn run(plat: &Platform) {
    println!("[Xtask] Launching CapsuleOS in QEMU...");

    // 1. Generate QEMU DTB dynamically
    println!("[Xtask] Generating QEMU DTB...");
    let mut dump_cmd = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    dump_cmd.args([
        "-M", "virt",
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
    println!("[Xtask] Running QEMU emulator (Ctrl+A, X to exit)");
    let mut qemu = Command::new(format!("qemu-system-{}", plat.qemu_arch));
    qemu.args([
        "-M", "virt",
        "-cpu", plat.qemu_cpu,
        "-m", plat.qemu_mem,
        "-nographic",
        "-kernel", &format!("build/target/{}/release/capsule-bootloader", plat.rust_target),
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
