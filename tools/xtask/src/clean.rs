use std::process::Command;
use crate::config::Resolved;

pub fn clean() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| format!("Failed to get cwd: {}", e))?;

    // 1. cargo clean (cleans build/target/ via .cargo/config.toml)
    println!("Cleaning cargo build artifacts...");
    let status = Command::new("cargo")
        .args(["clean"])
        .status()
        .map_err(|e| format!("Failed to run cargo clean: {}", e))?;
    if !status.success() {
        return Err("cargo clean failed".to_string());
    }

    // Try to load config. If it fails, fallback to defaults
    let mut dist_dir = "build/dist".to_string();
    let cache_dir = "build/cache".to_string();
    let rootfs_dir = "kernel/files".to_string();
    let toolchain_target = "tools/toolchain/target".to_string();

    if let Ok(resolved) = Resolved::load(None) {
        dist_dir = resolved.root.project.dist_dir().to_string();
    }

    // 2. Remove custom directories
    let dirs = [
        dist_dir,
        cache_dir,       // foreign-build cached outputs (bash, future autotools)
        rootfs_dir,
        toolchain_target,
    ];

    for dir in &dirs {
        let path = root.join(dir);
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("Failed to remove {}: {}", dir, e))?;
            println!("  Removed {}", dir);
        } else {
            println!("  Skipped {} (not found)", dir);
        }
    }

    // 3. Remove standalone Cargo.lock if present
    let cargo_lock = root.join("Cargo.lock");
    if cargo_lock.exists() {
        std::fs::remove_file(&cargo_lock)
            .map_err(|e| format!("Failed to remove Cargo.lock: {}", e))?;
        println!("  Removed Cargo.lock");
    }

    println!("Clean complete.");
    Ok(())
}
