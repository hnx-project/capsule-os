use crate::repo::{
    is_admin_mode, run_check_silent, run_cmd, run_cmd_status, verify_branch_name, XtaskConfig,
};
use std::fs;
use std::path::Path;

pub fn get_local_version() -> Result<String, String> {
    let manifest = fs::read_to_string("Cargo.toml")
        .map_err(|e| format!("Failed to read workspace Cargo.toml: {}", e))?;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version") {
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                return Ok(parts[1].trim().trim_matches('"').to_string());
            }
        }
    }
    Err("Could not find a valid version in workspace Cargo.toml".to_string())
}

pub fn get_upstream_version() -> Result<String, String> {
    let content = run_cmd(&["git", "show", "upstream/develop:Cargo.toml"], None).map_err(|_| {
        "Could not read upstream Cargo.toml. Ensure 'upstream' is added and fetched.".to_string()
    })?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version") {
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                return Ok(parts[1].trim().trim_matches('"').to_string());
            }
        }
    }
    Err("Could not parse version from upstream develop:Cargo.toml".to_string())
}

pub fn handle_commit(
    r#type: Option<&str>,
    scope: Option<&str>,
    message: Option<&str>,
    config: &XtaskConfig,
) -> Result<(), String> {
    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    verify_branch_name(&current_branch)?;

    let ohc_deps = [
        "kernel/files/init",
        "kernel/files/devmgr",
        "kernel/files/loader",
        "kernel/files/vfs",
    ];
    for file in &ohc_deps {
        let p = Path::new(file);
        if !p.exists() {
            if let Some(parent) = p.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(p, &[]);
        }
    }

    println!("🛠️ Running safety linter validation grid...");
    print!("🧹 Checking rustfmt... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    run_cmd_status(&["cargo", "fmt", "--check"], None).map_err(|_| {
        "\n❌ Formatting check failed! Please run 'cargo fmt' before committing.".to_string()
    })?;
    println!("\x1B[1;32m[OK]\x1B[0m");

    print!("🦀 Checking workspace compilation... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    run_check_silent(&["cargo", "check", "--workspace", "--quiet"], None)
        .map_err(|e| format!("\n❌ Workspace compilation failed! Details:{}", e))?;
    println!("\x1B[1;32m[OK]\x1B[0m");

    print!("🛡️ Checking AArch64 target compatibility... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    run_check_silent(
        &[
            "cargo",
            "check",
            "--manifest-path",
            "kernel/Cargo.toml",
            "--target",
            "aarch64-unknown-none",
            "--quiet",
        ],
        None,
    )
    .map_err(|e| {
        format!(
            "\n❌ AArch64 kernel compilation check failed! Details:{}",
            e
        )
    })?;
    println!("\x1B[1;32m[OK]\x1B[0m");

    print!("🛡️ Checking RISC-V 64 target compatibility... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    run_check_silent(
        &[
            "cargo",
            "check",
            "--manifest-path",
            "kernel/Cargo.toml",
            "--target",
            "riscv64imac-unknown-none-elf",
            "--quiet",
        ],
        None,
    )
    .map_err(|e| {
        format!(
            "\n❌ RISC-V 64 kernel compilation check failed! Details:{}",
            e
        )
    })?;
    println!("\x1B[1;32m[OK]\x1B[0m");

    let local_ver = get_local_version()?;
    if let Ok(upstream_ver) = get_upstream_version() {
        let diff =
            run_cmd(&["git", "diff", "--name-only", "upstream/develop"], None).unwrap_or_default();
        let source_changed = diff
            .lines()
            .any(|l| l.ends_with(".rs") || l.contains("Cargo.toml"));

        if source_changed && local_ver == upstream_ver {
            if is_admin_mode(config) {
                println!("ℹ️ \x1B[1;33mMaintainer/Release Mode Detected: Bypassing identical version safety gate.\x1B[0m");
            } else {
                return Err(format!(
                    "❌ Version Check Violation!\n\
                     Your local workspace version is identical to the upstream/develop version: '{}'.\n\
                     You modified source code/config. You MUST increment the version in Cargo.toml before committing.\n\
                     Example: Change to '{}-develop' or a higher semver bump.",
                    local_ver, local_ver
                ));
            }
        }
    }

    let commit_type = match r#type {
        Some(t) => t.to_string(),
        None => {
            println!(
                "💡 Enter commit type (e.g. feat, fix, chore, docs, refactor, style, test, perf):"
            );
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| e.to_string())?;
            input.trim().to_string()
        }
    };

    let commit_scope = match scope {
        Some(s) => Some(s.to_string()),
        None => {
            println!(
                "💡 Enter optional scope (press Enter to skip, e.g. kernel, std, bootloader):"
            );
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| e.to_string())?;
            let trimmed = input.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
    };

    let commit_msg = match message {
        Some(m) => m.to_string(),
        None => {
            println!("💡 Enter short, descriptive commit message:");
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| e.to_string())?;
            input.trim().to_string()
        }
    };

    if commit_type.is_empty() || commit_msg.is_empty() {
        return Err("Commit type and message description cannot be empty!".to_string());
    }

    let valid_types = [
        "feat", "fix", "chore", "docs", "refactor", "style", "test", "perf", "wip",
    ];
    if !valid_types.contains(&commit_type.as_str()) {
        return Err(format!(
            "Invalid commit type: '{}'. Valid types: {:?}",
            commit_type, valid_types
        ));
    }

    let final_message = match commit_scope {
        Some(sc) => format!("{}({}): {}", commit_type, sc, commit_msg),
        None => format!("{}: {}", commit_type, commit_msg),
    };

    println!("✍️ Committing to local git: \"{}\"", final_message);
    run_cmd_status(&["git", "commit", "-m", &final_message], None)
        .map_err(|e| format!("Failed to run git commit: {}", e))?;

    println!("✅ Code committed successfully and securely!");
    Ok(())
}
