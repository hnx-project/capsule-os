//! `Platform` is the composed view used by `build` / `run` / `test`.
//!
//! It maps the arch and platform to static constants representing memory mapped addresses,
//! while extracting dynamic runtime simulation variables from the unified configuration.

pub struct Platform {
    pub arch: String,
    #[allow(dead_code)]
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

    // QEMU run args (from [run.qemu] section in configuration)
    pub qemu_bin: String,
    pub qemu_args: Vec<String>,
    pub qemu_dtb_dump_args: Vec<String>,
    pub qemu_smp: u32,
    pub qemu_disk_img: String,
}

impl Platform {
    /// Build a `Platform` view from arch, platform profile name, and RootConfig metadata.
    pub fn from_configs(
        arch: &str,
        profile_name: &str,
        config: &crate::config::RootConfig,
    ) -> Option<Self> {
        // Enforce supported architecture
        if arch != "aarch64" {
            return None;
        }

        // Retrieve architectural target values (memory map addresses and linker variables)
        let (
            rust_target,
            userspace_target,
            kernel_entry,
            ld_emulation,
            linker_script,
            dtb_addr,
            ohc_addr,
            boot_addr,
            rootfs_addr,
        ) = match profile_name {
            "virt" => (
                "aarch64-unknown-none".to_string(),
                "libraries/targets/aarch64-unknown-capsule.json".to_string(),
                "1074266112".to_string(),
                "aarch64elf".to_string(),
                "kernel/linker/kernel_aarch64.ld".to_string(),
                "0x42000000".to_string(),
                "0x40700000".to_string(),
                "0x44000000".to_string(),
                "0x46000000".to_string(),
            ),
            "rpi" => (
                "aarch64-unknown-none".to_string(),
                "libraries/targets/aarch64-unknown-capsule.json".to_string(),
                "1074266112".to_string(),
                "aarch64elf".to_string(),
                "kernel/linker/kernel_aarch64.ld".to_string(),
                "0x42000000".to_string(),
                "0x44010000".to_string(),
                "0x44000000".to_string(),
                "0x46000000".to_string(),
            ),
            _ => return None,
        };

        // Extract runtime settings
        let (
            qemu_bin,
            qemu_args,
            qemu_dtb_dump_args,
            qemu_smp,
            qemu_disk_img,
        ) = config.run.qemu.as_ref()
            .map(|q| {
                (
                    q.bin.clone(),
                    q.args.clone(),
                    q.dtb_dump_args.clone(),
                    q.smp,
                    q.disk_img.clone(),
                )
            })
            .unwrap_or_else(|| {
                (
                    String::new(),
                    Vec::new(),
                    Vec::new(),
                    4,
                    crate::config::default_disk_img(),
                )
            });

        Some(Platform {
            arch: arch.to_string(),
            profile: profile_name.to_string(),
            rust_target,
            userspace_target,
            kernel_entry,
            ld_emulation,
            linker_script,
            dtb_addr,
            ohc_addr,
            boot_addr,
            rootfs_addr,
            qemu_bin,
            qemu_args,
            qemu_dtb_dump_args,
            qemu_smp,
            qemu_disk_img,
        })
    }
}
