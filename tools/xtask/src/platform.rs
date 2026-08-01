//! `Platform` is the composed view used by `build` / `run` / `test`.
//!
//! It pulls build addresses from `xtask.build.toml` and runtime args
//! from whichever `xtask.<platform>.toml` the user picked.  Fields
//! the active command doesn't need are `None` (e.g. RPI build has no
//! `qemu_*` fields).

use crate::config::{BuildConfig, RuntimeConfig};

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

    // QEMU run args (present iff the runtime-config had a [qemu] block).
    pub qemu_bin: String,
    pub qemu_args: Vec<String>,
    pub qemu_dtb_dump_args: Vec<String>,
    pub qemu_smp: u32,
    pub qemu_disk_img: String,
}

impl Platform {
    /// Build a `Platform` view from a build profile + (optionally) a
    /// matching runtime profile.  When the runtime profile has no
    /// QEMU block (e.g. `xtask.rpi.toml`) the qemu_* fields stay at
    /// their defaults and `run::run` should never try to launch QEMU.
    pub fn from_configs(
        arch: &str,
        profile_name: &str,
        build: &BuildConfig,
        runtime: &RuntimeConfig,
    ) -> Option<Self> {
        let arch_cfg = build.platform.get(arch)?;
        let p_cfg = arch_cfg.profiles.get(profile_name)?;

        let (
            qemu_bin,
            qemu_args,
            qemu_dtb_dump_args,
            qemu_smp,
            qemu_disk_img,
        ) = runtime
            .platform
            .get(arch)
            .and_then(|a| a.profiles.get(profile_name))
            .and_then(|p| p.qemu.as_ref())
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
                    1,
                    crate::config::default_disk_img(),
                )
            });

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
            qemu_bin,
            qemu_args,
            qemu_dtb_dump_args,
            qemu_smp,
            qemu_disk_img,
        })
    }
}