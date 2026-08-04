pub struct Platform {
    pub arch: String,
    pub profile: String,
    pub rust_target: String,
    pub userspace_target: String,
    pub kernel_entry: String,
    pub ld_emulation: String,
    pub linker_script: String,
    pub dtb_addr: String,
    pub ohc_addr: String,
    pub boot_addr: String,
    pub rootfs_addr: String,

    pub qemu_bin: String,
    pub qemu_args: Vec<String>,
    pub qemu_dtb_dump_args: Vec<String>,
    pub qemu_smp: u32,
    pub qemu_disk_img: String,
}

impl Platform {
    pub fn from_configs(
        arch: &str,
        profile_name: &str,
        config: &crate::config::RootConfig,
    ) -> Self {
        let qemu_bin = config.run.qemu.bin.clone();
        let qemu_args = config.run.qemu.args.clone();
        let qemu_dtb_dump_args = config.run.qemu.dtb_dump_args.clone();
        let qemu_smp = config.run.qemu.smp;
        let qemu_disk_img = config.run.qemu.disk_img.clone();

        let root_dir = std::env::current_dir().unwrap();
        let userspace_target_abs = root_dir.join("libraries/targets/aarch64-unknown-capsule.json")
            .to_string_lossy().to_string();
        let linker_script_abs = root_dir.join("kernel/linker/kernel_aarch64.ld")
            .to_string_lossy().to_string();

        Platform {
            arch: arch.to_string(),
            profile: profile_name.to_string(),
            rust_target: "aarch64-unknown-none".to_string(),
            userspace_target: userspace_target_abs,
            kernel_entry: "1074266112".to_string(),
            ld_emulation: "aarch64elf".to_string(),
            linker_script: linker_script_abs,
            dtb_addr: "0x42000000".to_string(),
            ohc_addr: "0x40700000".to_string(),
            boot_addr: "0x44000000".to_string(),
            rootfs_addr: "0x46000000".to_string(),
            qemu_bin,
            qemu_args,
            qemu_dtb_dump_args,
            qemu_smp,
            qemu_disk_img,
        }
    }
}
