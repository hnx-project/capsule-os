use serde::Deserialize;
use std::path::Path;
use std::fs;
use std::process::Command;

pub const BUILD_TARGET: &str = "build/target";
pub const BUILD_TEMP_RESOURCE: &str = "build/dist/temp_resource";
pub const BUILD_TEMP_EFI: &str = "build/dist/temp_efi";
pub const BUILD_TEMP_ROOTFS: &str = "build/dist/temp_rootfs";
pub const BUILD_DIST: &str = "build/dist";

#[derive(Debug, Deserialize, Clone)]
pub struct RootConfig {
    pub project: Project,
    pub build: BuildConfig,
    #[serde(default)]
    pub run: RunConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub codename: String,
}

impl Project {
    #[allow(dead_code)]
    pub fn dist_dir(&self) -> &str {
        BUILD_DIST
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct BuildConfig {
    pub bootloader: Option<BuildItem>,
    pub kernel: Option<BuildItem>,
    #[serde(default)]
    pub pillsmod: Vec<BuildItem>,
    #[serde(default)]
    pub libraries: Vec<BuildItem>,
    #[serde(default)]
    pub services: Vec<BuildItem>,
    #[serde(default)]
    pub apps: Vec<BuildItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BuildItem {
    pub path: String,
    pub output: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RunConfig {
    pub qemu: Option<QemuRuntime>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct QemuRuntime {
    pub bin: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub dtb_dump_args: Vec<String>,
    #[serde(default = "default_smp")]
    pub smp: u32,
    #[serde(default = "default_disk_img")]
    pub disk_img: String,
}

pub fn default_disk_img() -> String {
    "build/disk.img".to_string()
}

fn default_smp() -> u32 {
    4
}

pub fn resolve_path_placeholders(path: &str) -> String {
    path.replace("{BUILD_TARGET}", BUILD_TARGET)
        .replace("{BUILD_TEMP_RESOURCE}", BUILD_TEMP_RESOURCE)
        .replace("{BUILD_TEMP_EFI}", BUILD_TEMP_EFI)
        .replace("{BUILD_TEMP_ROOTFS}", BUILD_TEMP_ROOTFS)
        .replace("{BUILD_DIST}", BUILD_DIST)
}

/// Parse Cargo.toml to extract the package name and target binary name
pub fn parse_cargo_toml(path: &str) -> Result<(String, String), String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;
    
    let mut package_name = String::new();
    let mut bin_name = String::new();
    let mut in_package = false;
    let mut in_bin = false;
    
    for line in content.lines() {
        let line_trimmed = line.trim();
        if line_trimmed == "[package]" {
            in_package = true;
            in_bin = false;
        } else if line_trimmed == "[[bin]]" {
            in_package = false;
            in_bin = true;
        } else if line_trimmed.starts_with('[') {
            in_package = false;
            in_bin = false;
        }
        
        if in_package && line_trimmed.starts_with("name") {
            if let Some(idx) = line_trimmed.find('=') {
                let name = line_trimmed[idx + 1..].trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                package_name = name;
            }
        }
        
        if in_bin && line_trimmed.starts_with("name") {
            if let Some(idx) = line_trimmed.find('=') {
                let name = line_trimmed[idx + 1..].trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                bin_name = name;
            }
        }
    }
    
    if package_name.is_empty() {
        return Err(format!("Could not find package name in {}", path));
    }
    
    if bin_name.is_empty() {
        bin_name = package_name.replace("hnx-", "");
    }
    
    Ok((package_name, bin_name))
}

// ----------------------------------------------------------------------------
// Resolved: composed view for backwards compatibility with orchestrator files
// ----------------------------------------------------------------------------

pub struct Resolved {
    pub root: RootConfig,
    pub build: BuildConfig,
}

impl Resolved {
    pub fn load(
        _platform: &str,
        runtime_override: Option<&Path>,
    ) -> Result<Self, String> {
        let path = match runtime_override {
            Some(p) => p.to_path_buf(),
            None => Path::new("xtaskfile").to_path_buf(),
        };
        
        let root = read_toml::<RootConfig>(&path)?;
        let build = root.build.clone();
        
        Ok(Self { root, build })
    }
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> Result<T, String> {
    let p = path.as_ref();
    if !p.exists() {
        return Err(format!("config file not found: {}", p.display()));
    }
    let content = fs::read_to_string(p)
        .map_err(|e| format!("failed to read {}: {}", p.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("failed to parse {}: {}", p.display(), e))
}

// ----------------------------------------------------------------------------
// Versioning helpers
// ----------------------------------------------------------------------------

pub struct ParsedVersion {
    pub os_name: String,
    pub codename: String,
    pub major: u32,
    pub minor: u32,
    pub patch: String,
    pub tag: String,
}

impl ParsedVersion {
    pub fn to_display_string(&self) -> String {
        format!(
            "{} {} v{}.{}.{} ({})",
            self.os_name, self.codename, self.major, self.minor, self.patch, self.tag
        )
    }
}

pub fn get_parsed_version(config: &RootConfig) -> ParsedVersion {
    let os_name = config.project.name.clone();
    let codename = config.project.codename.clone();

    let mut major = 1;
    let mut minor = 0;

    let raw_ver = &config.project.version;
    let ver_parts: Vec<&str> = raw_ver.split('-').collect();
    let mut tag = if ver_parts.len() >= 2 {
        ver_parts[1].to_string()
    } else {
        "release".to_string()
    };

    let num_parts: Vec<&str> = ver_parts[0].split('.').collect();
    if num_parts.len() >= 1 {
        if let Ok(maj) = num_parts[0].parse::<u32>() {
            major = maj;
        }
    }
    if num_parts.len() >= 2 {
        if let Ok(min) = num_parts[1].parse::<u32>() {
            minor = min;
        }
    }

    let mut patch = "0".to_string();
    let mut git_success = false;

    let match_pattern = "v*".to_string();
    if let Ok(output) = Command::new("git")
        .args(["describe", "--tags", "--long", "--match", &match_pattern])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            let trimmed = s.trim();
            let parts: Vec<&str> = trimmed.split('-').collect();
            if parts.len() >= 3 {
                patch = parts[parts.len() - 2].to_string();
                let tag_info = &parts[..parts.len() - 2];
                if tag_info.len() >= 1 {
                    let clean_ver = tag_info[0].trim_start_matches('v');
                    let version_parts: Vec<&str> = clean_ver.split('.').collect();
                    if version_parts.len() >= 1 {
                        if let Ok(maj) = version_parts[0].parse::<u32>() {
                            major = maj;
                        }
                    }
                    if version_parts.len() >= 2 {
                        if let Ok(min) = version_parts[1].parse::<u32>() {
                            minor = min;
                        }
                    }
                }
                if tag_info.len() >= 2 {
                    tag = tag_info[1..].join("-");
                } else {
                    tag = "release".to_string();
                }
                git_success = true;
            }
        }
    }

    if !git_success {
        if let Ok(output) = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                patch = s.trim().to_string();
            }
        }
    }

    ParsedVersion {
        os_name,
        codename,
        major,
        minor,
        patch,
        tag,
    }
}
