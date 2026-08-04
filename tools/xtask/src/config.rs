use serde::Deserialize;
use std::path::Path;

pub const BUILD_TARGET: &str = "build/target";
pub const BUILD_TEMP_RESOURCE: &str = "build/dist/temp_resource";
pub const BUILD_DIST: &str = "build/dist";

pub const TOOLCHAIN_LINKER_PATH: &str = "tools/toolchain/Cargo.toml";
pub const TOOLCHAIN_LINKER_PACKAGE: &str = "ohlink-linker";

#[derive(Debug, Deserialize, Clone)]
pub struct RootConfig {
    pub project: Project,
    pub build: BuildSection,
    pub run: RunSection,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub codename: String,
    #[serde(default = "default_target_dir")]
    pub target_dir: String,
    #[serde(default)]
    pub dist_dir: Option<String>,
}

fn default_target_dir() -> String {
    "build/target".to_string()
}

impl Project {
    pub fn dist_dir(&self) -> &str {
        self.dist_dir.as_deref().unwrap_or("build/dist")
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct BuildSection {
    pub bootloader: BuildItem,
    pub kernel: BuildItem,
    pub pillsmod: Vec<BuildItem>,
    pub libraries: Vec<BuildItem>,
    pub services: Vec<BuildItem>,
    pub apps: Vec<BuildItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BuildItem {
    pub path: String,
    pub output: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RunSection {
    pub qemu: QemuConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct QemuConfig {
    pub bin: String,
    pub smp: u32,
    pub disk_img: String,
    pub args: Vec<String>,
    pub dtb_dump_args: Vec<String>,
}

pub struct Resolved {
    pub root: RootConfig,
}

impl Resolved {
    pub fn load(config_override: Option<&Path>) -> Result<Self, String> {
        let config_filename = match config_override {
            Some(p) => p.to_string_lossy().to_string(),
            None => {
                if Path::new("xtaskfile").exists() {
                    "xtaskfile".to_string()
                } else if Path::new("xtask.toml").exists() {
                    "xtask.toml".to_string()
                } else {
                    return Err("No xtaskfile or xtask.toml found in current directory".to_string());
                }
            }
        };

        let root = read_toml::<RootConfig>(&config_filename)?;
        Ok(Self { root })
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

#[allow(dead_code)]
pub type Config = RootConfig;
