use std::process::Command;
use crate::config::BUILD_DIST;

pub fn clean() -> Result<(), String> {
    let root = std::env::current_dir().map_err(|e| format!("Failed to get cwd: {}", e))?;

    // 1. cargo clean
    println!("Cleaning cargo build artifacts...");
    let status = Command::new("cargo")
        .args(["clean"])
        .status()
        .map_err(|e| format!("Failed to run cargo clean: {}", e))?;
    if !status.success() {
        return Err("cargo clean failed".to_string());
    }

    // 2. Remove standard build output files and directories
    let dirs = [
        BUILD_DIST.to_string(),
        "build/cache".to_string(),
        "kernel/files/loader.img".to_string(),
        "kernel/files/rootfs.img".to_string(),
        "tools/ohlink-toolchain/target".to_string(),
    ];

    for dir in &dirs {
        let path = root.join(dir);
        if path.exists() {
            if path.is_file() {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("Failed to remove file {}: {}", dir, e))?;
            } else {
                std::fs::remove_dir_all(&path)
                    .map_err(|e| format!("Failed to remove dir {}: {}", dir, e))?;
            }
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
