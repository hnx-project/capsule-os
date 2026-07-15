use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub project: Project,
    pub gitcode: GitCode,
    pub submodules: BTreeMap<String, Submodule>,
    pub toolchain: Toolchain,
    pub platform: BTreeMap<String, PlatformConfig>,
    pub subprojects: Vec<Subproject>,
    pub distribution: Distribution,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Project {
    pub name: String,
    pub codename: String,
    pub version: String,
    pub target_dir: String,
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
pub struct PlatformConfig {
    pub rust_target: String,
    pub userspace_target: String,
    pub kernel_entry: String,
    pub ld_emulation: String,
    pub linker_script: String,
    pub dtb_addr: String,
    pub ohc_addr: String,
    pub boot_addr: String,
    pub rootfs_addr: String,
    pub qemu: QemuConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct QemuConfig {
    pub bin: String,
    pub args: Vec<String>,
    pub dtb_dump_args: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Subproject {
    pub name: String,
    #[serde(rename = "type")]
    pub subproject_type: String,
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserCrate {
    pub crate_name: String,
    pub out_name: String,
    pub entry: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Distribution {
    pub output_dir: String,
    pub image_name_template: String,
    pub stages: Vec<DistributionStage>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DistributionStage {
    pub input: String,
    pub pad_to: Option<u64>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let path = Path::new("xtask.toml");
        if !path.exists() {
            return Err("xtask.toml config file not found".to_string());
        }
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let config: Config =
            toml::from_str(&content).map_err(|e| format!("failed to parse xtask.toml: {}", e))?;
        Ok(config)
    }
}
