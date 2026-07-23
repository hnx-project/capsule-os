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
    /// Effective guest CPU count (also seeded into `dumpdtb`).
    /// `0` means "not a qemu profile" or "default to 1".
    pub qemu_smp: u32,
}

impl Platform {
    pub fn from_config(arch: &str, profile_name: &str, config: &crate::config::Config) -> Option<Self> {
        let arch_cfg = config.platform.get(arch)?;
        let p_cfg = arch_cfg.profiles.get(profile_name)?;
        Some(Platform {
            arch: arch.to_string(),
            profile: profile_name.to_string(),
            rust_target: p_cfg.rust_target.clone(),
            userspace_target: p_cfg.userspace_target.clone(),
            kernel_entry: p_cfg.kernel_entry.clone(),
            ld_emulation: p_cfg.ld_emulation.clone(),
            linker_script: p_cfg.linker_script.clone(),
            dtb_addr: p_cfg.dtb_addr.clone(),
            ohc_addr: p_cfg.ohc_addr.clone(),
            boot_addr: p_cfg.boot_addr.clone(),
            rootfs_addr: p_cfg.rootfs_addr.clone(),
            qemu_bin: p_cfg.qemu.as_ref().map(|q| q.bin.clone()).unwrap_or_default(),
            qemu_args: p_cfg.qemu.as_ref().map(|q| q.args.clone()).unwrap_or_default(),
            qemu_dtb_dump_args: p_cfg.qemu.as_ref().map(|q| q.dtb_dump_args.clone()).unwrap_or_default(),
            qemu_smp: p_cfg.qemu.as_ref().map(|q| q.smp).unwrap_or(1),
        })
    }
}
