use std::fs;
use std::path::Path;
use crate::config::Config;

pub fn check_version(sync: bool, config: &Config) -> Result<(), String> {
    let xtask_version = &config.project.version;
    println!("🔍 [Version Check] xtask.toml configured version: \x1b[1;36m{}\x1b[0m", xtask_version);

    // 1. Read root Cargo.toml
    let root_cargo_path = Path::new("Cargo.toml");
    if !root_cargo_path.exists() {
        return Err("Root Cargo.toml not found!".to_string());
    }
    let mut root_content = fs::read_to_string(root_cargo_path)
        .map_err(|e| format!("Failed to read root Cargo.toml: {}", e))?;

    // Find the version under [workspace.package]
    let mut workspace_version = None;
    let mut in_workspace_package = false;
    let lines: Vec<&str> = root_content.lines().collect();
    for line in &lines {
        let trimmed = line.trim();
        if trimmed == "[workspace.package]" {
            in_workspace_package = true;
            continue;
        } else if trimmed.starts_with('[') {
            in_workspace_package = false;
        }

        if in_workspace_package && trimmed.starts_with("version") {
            if let Some(eq_idx) = trimmed.find('=') {
                let val = trimmed[eq_idx + 1..].trim().trim_matches('"');
                workspace_version = Some(val.to_string());
                break;
            }
        }
    }

    let current_workspace_version = match workspace_version {
        Some(v) => v,
        None => return Err("Could not find [workspace.package] version in root Cargo.toml".to_string()),
    };
    println!("🔍 [Version Check] Root Cargo.toml workspace version: \x1b[1;36m{}\x1b[0m", current_workspace_version);

    let mut versions_match = current_workspace_version == *xtask_version;

    if !versions_match {
        if sync {
            println!("📥 [Version Sync] Synchronizing root Cargo.toml workspace version to \x1b[1;32m{}\x1b[0m...", xtask_version);
            let mut new_content = String::new();
            let mut in_workspace_package = false;
            for line in root_content.lines() {
                let trimmed = line.trim();
                if trimmed == "[workspace.package]" {
                    in_workspace_package = true;
                    new_content.push_str(line);
                    new_content.push('\n');
                    continue;
                } else if trimmed.starts_with('[') {
                    in_workspace_package = false;
                }

                if in_workspace_package && trimmed.starts_with("version") {
                    if let Some(_eq_idx) = line.find('=') {
                        let indent = &line[..line.find("version").unwrap()];
                        new_content.push_str(&format!("{}version = \"{}\"\n", indent, xtask_version));
                        continue;
                    }
                }
                new_content.push_str(line);
                new_content.push('\n');
            }
            fs::write(root_cargo_path, &new_content)
                .map_err(|e| format!("Failed to write updated root Cargo.toml: {}", e))?;
            root_content = new_content; // Update root_content for parsing members correctly
            versions_match = true;
            println!("✨ [Version Sync] Root Cargo.toml successfully synchronized!");
        } else {
            println!("\x1b[1;31m❌ Error: Version mismatch! xtask.toml ({}) differs from root Cargo.toml ({})\x1b[0m", xtask_version, current_workspace_version);
            println!("💡 Run \x1b[1;36mxtask code check-version --sync\x1b[0m to synchronize them automatically.");
        }
    }

    // 2. Scan all workspace members
    println!("🔍 [Version Check] Scanning workspace members for standard version inheritance...");
    let mut scan_errors = 0;

    // Parse workspace members from root Cargo.toml
    let mut members = Vec::new();
    let mut in_members = false;
    for line in root_content.lines() {
        let trimmed = line.trim();
        if trimmed == "members = [" {
            in_members = true;
            continue;
        } else if in_members && trimmed == "]" {
            in_members = false;
            continue;
        }

        if in_members {
            let clean_member = trimmed.trim_matches(|c| c == '"' || c == ',' || c == ' ' || c == '\'');
            if !clean_member.is_empty() && !clean_member.starts_with('#') {
                members.push(clean_member.to_string());
            }
        }
    }

    for member in &members {
        if member.starts_with("tools/ohlink-toolchain") {
            continue;
        }
        let member_cargo_path = Path::new(member).join("Cargo.toml");
        if !member_cargo_path.exists() {
            println!("\x1b[1;33m⚠️ Warning: Member path '{}' does not have a Cargo.toml!\x1b[0m", member);
            continue;
        }

        let member_content = fs::read_to_string(&member_cargo_path)
            .map_err(|e| format!("Failed to read member Cargo.toml for {}: {}", member, e))?;

        // Check if member has standard package version workspace inheritance
        let mut has_workspace_inheritance = false;
        let mut package_version_line = None;
        let mut in_package = false;

        for (idx, line) in member_content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed == "[package]" {
                in_package = true;
                continue;
            } else if trimmed.starts_with('[') {
                in_package = false;
            }

            if in_package && trimmed.starts_with("version") {
                package_version_line = Some((idx, line.to_string()));
                if trimmed.contains("workspace") && trimmed.contains("true") {
                    has_workspace_inheritance = true;
                }
                break;
            }
        }

        if !has_workspace_inheritance {
            if sync {
                if let Some(_) = package_version_line {
                    println!("📥 [Version Sync] Migrating member \x1b[1;36m{}\x1b[0m Cargo.toml to use workspace version...", member);
                    let mut new_member_content = String::new();
                    let mut in_package = false;
                    for line in member_content.lines() {
                        let trimmed = line.trim();
                        if trimmed == "[package]" {
                            in_package = true;
                            new_member_content.push_str(line);
                            new_member_content.push('\n');
                            continue;
                        } else if trimmed.starts_with('[') {
                            in_package = false;
                        }

                        if in_package && trimmed.starts_with("version") {
                            let indent = &line[..line.find("version").unwrap()];
                            new_member_content.push_str(&format!("{}version.workspace = true\n", indent));
                            continue;
                        }
                        new_member_content.push_str(line);
                        new_member_content.push('\n');
                    }
                    fs::write(&member_cargo_path, &new_member_content)
                        .map_err(|e| format!("Failed to write updated member Cargo.toml for {}: {}", member, e))?;
                    println!("✨ [Version Sync] \x1b[1;32m{}\x1b[0m Cargo.toml successfully migrated!", member);
                } else {
                    println!("\x1b[1;33m⚠️ Warning: Member '{}' does not have a version field under [package]!\x1b[0m", member);
                }
            } else {
                scan_errors += 1;
                println!("\x1b[1;31m❌ Error: Workspace member '{}' has a hardcoded version or is missing version.workspace = true!\x1b[0m", member);
                if let Some((_, ref old_line)) = package_version_line {
                    println!("   Found hardcoded version line: '{}'", old_line.trim());
                }
            }
        }
    }

    // 3. Independent Kernel Version check
    let kernel_cargo_path = Path::new("kernel/Cargo.toml");
    if kernel_cargo_path.exists() {
        let kernel_content = fs::read_to_string(kernel_cargo_path)
            .map_err(|e| format!("Failed to read kernel Cargo.toml: {}", e))?;
        let mut kernel_version = None;
        let mut in_package = false;
        for line in kernel_content.lines() {
            let trimmed = line.trim();
            if trimmed == "[package]" {
                in_package = true;
                continue;
            } else if trimmed.starts_with('[') {
                in_package = false;
            }

            if in_package && trimmed.starts_with("version") {
                if let Some(eq_idx) = trimmed.find('=') {
                    kernel_version = Some(trimmed[eq_idx + 1..].trim().trim_matches('"').to_string());
                    break;
                }
            }
        }
        if let Some(v) = kernel_version {
            println!("🔍 [Version Check] Subtree kernel (HNX) independent version: \x1b[1;35m{}\x1b[0m (tracked via kernel/Cargo.toml)", v);
        }
    }

    if scan_errors > 0 {
        return Err(format!("Version validation failed: {} member crate(s) are out of sync or hardcoded. Run 'xtask code check-version --sync' to fix.", scan_errors));
    }

    if !versions_match {
        return Err("Version validation failed: xtask.toml and root Cargo.toml are out of sync.".to_string());
    }

    println!("🎉 \x1b[1;32mAll version and workspace inheritance checks passed successfully!\x1b[0m");
    Ok(())
}