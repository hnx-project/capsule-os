use crate::repo::{run_check_silent, run_cmd, run_cmd_status, verify_branch_name, XtaskConfig};
use std::path::Path;

pub fn handle_push(config: &XtaskConfig) -> Result<(), String> {
    // 1. Get current branch and check naming convention for main repo
    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    verify_branch_name(&current_branch)?;

    // 2. Cascade check and push modified submodules automatically!
    println!("📦 Checking submodules for unpushed commits...");
    for (local_path, _sub_cfg) in &config.submodules {
        let sub_dir = Path::new(local_path);
        if sub_dir.exists() {
            // Check if there are modified or unpushed commits in the submodule relative to origin
            let submodule_branch =
                run_cmd(&["git", "branch", "--show-current"], Some(sub_dir)).unwrap_or_default();
            if !submodule_branch.is_empty() {
                // Check if submodule local branch is ahead of its origin (needs push)
                let unpushed = run_cmd(
                    &["git", "log", &format!("origin/{}..HEAD", submodule_branch)],
                    Some(sub_dir),
                )
                .unwrap_or_default();
                let is_dirty = !run_cmd(&["git", "status", "--porcelain"], Some(sub_dir))
                    .unwrap_or_default()
                    .trim()
                    .is_empty();

                if !unpushed.trim().is_empty() || is_dirty {
                    println!(
                        "📥 Unpushed changes detected in submodule '{}' (branch: {})!",
                        local_path, submodule_branch
                    );

                    if is_dirty {
                        println!(
                            "🌿 Staging and committing dirty files in submodule '{}'...",
                            local_path
                        );
                        run_cmd_status(&["git", "add", "."], Some(sub_dir))?;
                        let _ = run_cmd_status(
                            &[
                                "git",
                                "commit",
                                "-m",
                                "chore: auto sync submodule state before main push",
                            ],
                            Some(sub_dir),
                        );
                    }

                    println!(
                        "📤 Cascading push for submodule '{}' to origin/{}...",
                        local_path, submodule_branch
                    );
                    run_cmd_status(&["git", "push", "origin", &submodule_branch], Some(sub_dir)).map_err(|e| {
                        format!("❌ Failed to push submodule '{}' to origin: {}\n\
                                 Please make sure you have run 'cargo xtask repo setup-fork' and configured your fork repositories correctly.", local_path, e)
                    })?;
                    println!("✅ Submodule '{}' successfully pushed.", local_path);
                }
            }
        }
    }

    // 3. Run safety validation compilation checks on main repo
    println!("🛠️  Running safety linter validation grid on main repository...");
    println!("🧹 [1/2] Verifying compilation on AArch64 target...");
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
    )?;

    println!("🧹 [2/2] Verifying compilation on RISC-V 64 target...");
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
    )?;

    // 4. Safe push main repository branch to developer fork (origin)
    println!(
        "📤 Pushing main repository current branch to origin/{}...",
        current_branch
    );
    run_cmd_status(&["git", "push", "origin", &current_branch], None)
        .map_err(|e| format!("Failed to push to origin remote repository: {}", e))?;

    println!(
        "\n🎉 \x1B[1;32mSuccessfully pushed main repository and all modified submodules cleanly to your developer forks!\x1B[0m\n"
    );
    Ok(())
}
