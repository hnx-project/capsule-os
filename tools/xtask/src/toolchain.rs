use std::path::PathBuf;
use std::process::Command;

pub struct ToolchainInfo {
    pub rustc_version: String,
    pub rust_lld_path: PathBuf,
    pub llvm_objcopy_path: PathBuf,
}

pub fn check_toolchain(expected_rust: Option<&str>) -> Result<ToolchainInfo, String> {
    let rustc_version = get_rustc_version()?;
    if let Some(expected) = expected_rust {
        if !rustc_version.contains(expected) {
            eprintln!("Warning: Rust version mismatch. Expected: {}, Found: {}", expected, rustc_version);
        }
    }
    let rust_lld_path = find_rust_lld();
    let llvm_objcopy_path = find_objcopy();
    Ok(ToolchainInfo {
        rustc_version,
        rust_lld_path,
        llvm_objcopy_path,
    })
}

fn get_rustc_version() -> Result<String, String> {
    let output = Command::new("rustc")
        .args(["--version"])
        .output()
        .map_err(|e| format!("Failed to run rustc: {}", e))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn find_rust_lld() -> PathBuf {
    if let Ok(sysroot) = std::str::from_utf8(
        &Command::new("rustc").args(["--print", "sysroot"]).output().unwrap().stdout
    ) {
        let rustlib = PathBuf::from(sysroot.trim()).join("lib").join("rustlib");
        if let Ok(dirs) = std::fs::read_dir(rustlib) {
            for entry in dirs.flatten() {
                if entry.path().is_dir() {
                    let lld = entry.path().join("bin").join("rust-lld");
                    if lld.exists() {
                        return lld;
                    }
                }
            }
        }
    }
    PathBuf::from("rust-lld")
}

pub fn find_objcopy() -> PathBuf {
    let hb_objcopy = PathBuf::from("/opt/homebrew/opt/llvm/bin/llvm-objcopy");
    if hb_objcopy.exists() {
        return hb_objcopy;
    }
    if let Ok(sysroot) = std::str::from_utf8(
        &Command::new("rustc").args(["--print", "sysroot"]).output().unwrap().stdout
    ) {
        let rustlib = PathBuf::from(sysroot.trim()).join("lib").join("rustlib");
        if let Ok(dirs) = std::fs::read_dir(rustlib) {
            for entry in dirs.flatten() {
                if entry.path().is_dir() {
                    let objcopy = entry.path().join("bin").join("llvm-objcopy");
                    if objcopy.exists() {
                        return objcopy;
                    }
                }
            }
        }
    }
    PathBuf::from("llvm-objcopy")
}
