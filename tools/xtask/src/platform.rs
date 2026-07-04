pub struct Platform {
    pub arch: &'static str,
    pub rust_target: &'static str,
    pub kernel_entry: &'static str,
    pub ld_emulation: &'static str,
    pub linker_script: &'static str,
    pub qemu_arch: &'static str,
    pub qemu_cpu: &'static str,
    pub qemu_mem: &'static str,
    pub qemu_extra: Vec<&'static str>,
    pub dtb_addr: &'static str,
    pub ohc_addr: &'static str,
    pub boot_addr: &'static str,
}

impl Platform {
    pub fn for_arch(arch: &str) -> Option<Self> {
        match arch {
            "aarch64" => Some(Platform {
                arch: "aarch64",
                rust_target: "aarch64-unknown-none",
                kernel_entry: "1074266112",
                ld_emulation: "aarch64elf",
                linker_script: "kernel/linker/kernel_aarch64.ld",
                qemu_arch: "aarch64",
                qemu_cpu: "cortex-a72",
                qemu_mem: "512M",
                qemu_extra: vec![],
                dtb_addr: "0x42000000",
                ohc_addr: "0x40700000",
                boot_addr: "0x44000000",
            }),
            "riscv64" => Some(Platform {
                arch: "riscv64",
                rust_target: "riscv64imac-unknown-none-elf",
                kernel_entry: "2148007936",
                ld_emulation: "elf64lriscv",
                linker_script: "kernel/linker/kernel_riscv64.ld",
                qemu_arch: "riscv64",
                qemu_cpu: "rv64",
                qemu_mem: "512M",
                qemu_extra: vec!["-bios", "default"],
                dtb_addr: "0x82000000",
                ohc_addr: "0x80700000",
                boot_addr: "0x80200000",
            }),
            _ => None,
        }
    }
}
