pub struct Platform {
    pub arch: String,
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
}

impl Platform {
    pub fn from_config(arch: &str, config: &crate::config::Config) -> Option<Self> {
        let p_cfg = config.platform.get(arch)?;
        Some(Platform {
            arch: arch.to_string(),
            rust_target: p_cfg.rust_target.clone(),
            userspace_target: p_cfg.userspace_target.clone(),
            kernel_entry: p_cfg.kernel_entry.clone(),
            ld_emulation: p_cfg.ld_emulation.clone(),
            linker_script: p_cfg.linker_script.clone(),
            dtb_addr: p_cfg.dtb_addr.clone(),
            ohc_addr: p_cfg.ohc_addr.clone(),
            boot_addr: p_cfg.boot_addr.clone(),
            rootfs_addr: p_cfg.rootfs_addr.clone(),
            qemu_bin: p_cfg.qemu.bin.clone(),
            qemu_args: p_cfg.qemu.args.clone(),
            qemu_dtb_dump_args: p_cfg.qemu.dtb_dump_args.clone(),
        })
    }
}
