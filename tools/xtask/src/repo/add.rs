use crate::repo::{run_cmd, run_cmd_status, XtaskConfig};
use regex::Regex;
use std::fs;
use std::path::Path;

pub fn handle_add(files: &[String], config: &XtaskConfig) -> Result<(), String> {
    println!("🔍 [1/3] Performing secret leak vulnerability scan on modified files...");

    let changed_files = run_cmd(&["git", "diff", "--cached", "--name-only"], None)
        .unwrap_or_default()
        + "\n"
        + &run_cmd(&["git", "diff", "--name-only"], None).unwrap_or_default()
        + "\n"
        + &run_cmd(&["git", "status", "--porcelain"], None).unwrap_or_default();

    let secret_patterns = [
        r"(?i)token\s*=\s*['\x22][a-zA-Z0-9_-]{20,}['\x22]",
        r"(?i)gitcode_token\s*=\s*['\x22][a-zA-Z0-9_-]{20,}['\x22]",
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    ];

    for file_line in changed_files.lines() {
        let f_path = file_line
            .trim()
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim();
        if f_path.is_empty()
            || f_path == "xtask.toml"
            || f_path.contains("target/")
            || f_path.contains("Cargo.lock")
        {
            continue;
        }

        let path = Path::new(f_path);
        if path.is_file() {
            if let Ok(content) = fs::read_to_string(path) {
                for pattern in &secret_patterns {
                    let re = Regex::new(pattern).unwrap();
                    if re.is_match(&content) {
                        return Err(format!(
                            "❌ \x1B[1;31mSecurity Guard Violation!\x1B[0m\n\
                             Suspected Personal Access Token or Private Key leak detected in file: '{}'!\n\
                             Staging blocked to prevent accidental push to public remote.",
                            f_path
                        ));
                    }
                }
            }
        }
    }
    println!("✅ No secret leaks detected. Workspace content is safe and secure!");

    println!("🌿 [2/3] Staging main repository files...");
    let mut args = vec!["git", "add"];
    for f in files {
        args.push(f);
    }
    run_cmd_status(&args, None).map_err(|e| format!("Failed to stage main repository: {}", e))?;
    println!("✅ Main repository files successfully staged.");

    println!("📦 [3/3] Cascading stage check to submodules...");
    for (local_path, _sub_cfg) in &config.submodules {
        let sub_dir = Path::new(local_path);
        if sub_dir.exists() {
            let status =
                run_cmd(&["git", "status", "--porcelain"], Some(sub_dir)).unwrap_or_default();
            if !status.trim().is_empty() {
                println!(
                    "📥 Dirty submodule '{}' detected! Cascading stage...",
                    local_path
                );
                run_cmd_status(&["git", "add", "."], Some(sub_dir)).map_err(|e| {
                    format!("Failed to cascade stage submodule '{}': {}", local_path, e)
                })?;
                println!("✅ Submodule '{}' successfully staged.", local_path);
            }
        }
    }

    println!("\n🚀 \x1B[1;32mStaging completed perfectly across main repository and all modified submodules!\x1B[0m\n");
    Ok(())
}
