//! # xtask configuration split
//!
//! xtask's configuration is split across four files for single-responsibility
//! cleanliness:
//!
//! | File             | Holds                                                   |
//! |------------------|---------------------------------------------------------|
//! | `xtask.toml`     | project metadata, gitcode remotes, toolchain bootstrap, distribution template |
//! | `xtask.build.toml` | per-arch / per-platform build addresses + `[[subprojects]]` table |
//! | `xtask.qemu.toml` | QEMU-only run arguments (machine model, CPU, RAM, drives) |
//! | `xtask.rpi.toml`  | Raspberry Pi firmware cache + disk-image layout |
//!
//! `Resolved::load(arch, platform, runtime_config_override)` reads the
//! three files and produces a [`Resolved`] struct that the rest of
//! xtask consumes.  `--config <path>` (passed through CLI) overrides
//! the runtime-config file (the one named by `platform`).
//!
//! The legacy `Config` struct is kept for callers that only need root
//! metadata; new code should pick the narrowest struct for its needs.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

// ----------------------------------------------------------------------------
// Root metadata (`xtask.toml`)
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RootConfig {
    pub project: Project,
    pub gitcode: GitCode,
    pub submodules: BTreeMap<String, Submodule>,
    pub toolchain: Toolchain,
    pub distribution: Distribution,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Project {
    pub name: String,
    pub codename: String,
    pub version: String,
    pub target_dir: String,
    pub dist_dir: Option<String>,
}

impl Project {
    pub fn dist_dir(&self) -> &str {
        self.dist_dir.as_deref().unwrap_or("build/dist")
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct GitCode {
    pub upstream_owner: String,
    pub upstream_repo: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Submodule {
    pub upstream: String,
    pub fork_repo: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Toolchain {
    pub bootstrap: Vec<BootstrapItem>,
    pub linker: Linker,
    #[serde(default)]
    pub c_bindings: Option<CBindingsConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CBindingsConfig {
    pub crate_path: String,
    pub config_path: String,
    pub output_header: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapItem {
    pub name: String,
    pub path: String,
    pub release: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Linker {
    pub path: String,
    pub package: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Distribution {
    pub output_dir: String,
    pub image_name_template: String,
    pub dtb_dir: Option<String>,
    // Stages only live in the root config because they apply to both
    // QEMU and RPI final-image generation.
    #[serde(default)]
    pub stages: Vec<DistributionStage>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct DistributionStage {
    pub input: String,
    pub pad_to: Option<u64>,
}

// ----------------------------------------------------------------------------
// Build config (`xtask.build.toml`)
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct BuildConfig {
    /// Keyed by arch → profile name → per-platform build settings.
    pub platform: BTreeMap<String, ArchBuildConfig>,
    pub subprojects: Vec<Subproject>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ArchBuildConfig {
    pub profiles: BTreeMap<String, PlatformBuildProfile>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PlatformBuildProfile {
    pub rust_target: String,
    pub userspace_target: String,
    pub kernel_entry: String,
    pub ld_emulation: String,
    pub linker_script: String,
    pub dtb_addr: String,
    pub ohc_addr: String,
    pub boot_addr: String,
    pub rootfs_addr: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Subproject {
    pub name: String,
    #[serde(rename = "type")]
    pub subproject_type: String,
    /// `enable = false` lets users opt out of slow subprojects (e.g.
    /// autotools-based foreign builds) without editing this file.
    #[serde(default = "default_enable")]
    pub enable: bool,
    // For userspace apps
    pub crates: Option<Vec<UserCrate>>,
    pub staging_bin_dir: Option<String>,
    pub rootfs_output: Option<String>,
    pub etc_source: Option<String>,
    pub etc_target: Option<String>,
    // For kernel / bootloader / common crates
    pub path: Option<String>,
    pub package: Option<String>,
    pub link_output: Option<Option<String>>,
    pub raw_output: Option<Option<String>>,
    pub ohc_output: Option<Option<String>>,
    pub bin_output: Option<Option<String>>,
    // For foreign (autotools / cmake / gnu-make) subprojects.
    pub src_path: Option<String>,
    pub configure: Option<String>,
    pub build: Option<String>,
    pub artifact: Option<String>,
    pub ohlink_entry: Option<String>,
    pub staging_bin: Option<String>,
}

fn default_enable() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserCrate {
    pub crate_name: String,
    pub out_name: String,
    pub entry: String,
    pub config_path: Option<String>,
}

// ----------------------------------------------------------------------------
// Runtime config (`xtask.qemu.toml` or `xtask.rpi.toml`)
// ----------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct RuntimeConfig {
    pub platform: BTreeMap<String, ArchRuntimeConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ArchRuntimeConfig {
    pub profiles: BTreeMap<String, PlatformRuntimeProfile>,
}

/// One profile's runtime settings.  QEMU and RPI share the wrapper
/// but populate it differently (mutually-exclusive inner tables).
///
/// `broadcom` / `disk` are populated by `xtask.rpi.toml` and will be
/// consumed by the `code run --platform rpi` + disk-image arms of
/// `code build --platform rpi` (follow-up releases).  The fields are
/// reserved here so the schema is stable from day one.
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct PlatformRuntimeProfile {
    /// QEMU launcher (when platform = virt).
    #[serde(default)]
    pub qemu: Option<QemuRuntime>,
    /// Raspberry Pi boot flow (when platform = rpi).
    #[serde(default)]
    pub broadcom: Option<BroadcomRuntime>,
    /// Raspberry Pi disk-image generation.
    #[serde(default)]
    pub disk: Option<RpiDiskRuntime>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct QemuRuntime {
    pub bin: String,
    pub args: Vec<String>,
    pub dtb_dump_args: Vec<String>,
    /// Number of guest CPUs exposed both to the running VM and to
    /// the `dumpdtb` machinery.  Defaults to 1 if omitted.
    #[serde(default = "default_smp")]
    pub smp: u32,
    /// Path to the raw virtio block image that QEMU's `-drive file=…`
    /// points at.  xtask generates a fresh FAT12 image at this path
    /// before launching QEMU if the file is missing, so the build/run
    /// pipeline no longer depends on a stale `disk.img` sitting in the
    /// repository root.  Templated into `args` as `{disk_img}`.
    #[serde(default = "default_disk_img")]
    pub disk_img: String,
}

pub fn default_disk_img() -> String {
    "build/disk.img".to_string()
}

fn default_smp() -> u32 {
    1
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct BroadcomRuntime {
    pub firmware_cache: String,
    #[serde(default)]
    pub firmware: Vec<BroadcomFirmwareEntry>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct BroadcomFirmwareEntry {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct RpiDiskRuntime {
    pub output_dir: String,
    pub image_arch: String,
    pub bootloader_pad_bytes: u64,
    #[serde(default)]
    pub stages: Vec<DistributionStage>,
}

// ----------------------------------------------------------------------------
// Resolved: composed view the rest of xtask consumes.
// ----------------------------------------------------------------------------

/// The three parsed TOML configs.  Code paths that need only root
/// metadata take `&RootConfig`; those that need build addresses take
/// `&BuildConfig` + a [`crate::platform::Platform`]; those that need
/// run-only data take `&RuntimeConfig`.
pub struct Resolved {
    pub root: RootConfig,
    pub build: BuildConfig,
    pub runtime: RuntimeConfig,
}

impl Resolved {
    /// Read `xtask.toml`, `xtask.build.toml`, and the runtime-config
    /// file appropriate to `platform` (`xtask.qemu.toml` for virt,
    /// `xtask.rpi.toml` for rpi).
    ///
    /// `runtime_override`, if `Some`, replaces the platform-default
    /// runtime-config file.
    pub fn load(
        platform: &str,
        runtime_override: Option<&Path>,
    ) -> Result<Self, String> {
        let root = read_toml::<RootConfig>("xtask.toml")?;
        let build = read_toml::<BuildConfig>("xtask.build.toml")?;

        let runtime_path: std::path::PathBuf = match runtime_override {
            Some(p) => p.to_path_buf(),
            None => default_runtime_path(platform)?,
        };
        let runtime = read_toml::<RuntimeConfig>(&runtime_path)?;

        Ok(Self { root, build, runtime })
    }
}

fn default_runtime_path(platform: &str) -> Result<std::path::PathBuf, String> {
    match platform {
        "virt" => Ok("xtask.qemu.toml".into()),
        "rpi" => Ok("xtask.rpi.toml".into()),
        other => Err(format!(
            "unknown platform '{other}': expected 'virt' or 'rpi'"
        )),
    }
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T, String> {
    let p = path.as_ref();
    if !p.exists() {
        return Err(format!("config file not found: {}", p.display()));
    }
    let content = std::fs::read_to_string(p)
        .map_err(|e| format!("failed to read {}: {}", p.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("failed to parse {}: {}", p.display(), e))
}

// ----------------------------------------------------------------------------
// Legacy `Config` shim.
//
// Older call sites take `&Config` for root-only fields (project name,
// toolchain bootstrap, distribution).  `Config` is now a thin alias
// over `RootConfig` so existing signatures continue to compile.
// ----------------------------------------------------------------------------

#[deprecated(
    since = "Pangu 1.0.0.beta5",
    note = "Use `Resolved::load()` and pick the narrowest struct (RootConfig / BuildConfig / RuntimeConfig)"
)]
#[allow(dead_code)]
pub type Config = RootConfig;