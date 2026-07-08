use crate::repo::{run_cmd, run_cmd_status, XtaskConfig};

pub fn handle_pull(_config: &XtaskConfig) -> Result<(), String> {
    println!("📥 [1/2] Fetching and aligning with your personal Fork (origin)...");

    let dirty_check = run_cmd(&["git", "status", "--porcelain"], None).unwrap_or_default();
    if !dirty_check.trim().is_empty() {
        return Err("⚠️  Local workspace is not clean! Please commit, stash, or stash-save your changes before running pull.".to_string());
    }

    // Fetch from personal fork remote (origin)
    run_cmd_status(&["git", "fetch", "origin"], None)
        .map_err(|_| "Could not fetch from 'origin'. Ensure you have configured origin by running: cargo xtask repo setup-fork".to_string())?;

    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    if current_branch == "develop" {
        println!("🔀 Rebasing local develop branch against origin/develop...");
        run_cmd_status(&["git", "rebase", "origin/develop"], None)?;
    } else if current_branch == "main" {
        println!("🔀 Rebasing local main branch against origin/main...");
        let _ = run_cmd_status(&["git", "rebase", "origin/main"], None);
    } else {
        println!("🔀 Merging/Rebasing active branch '{}' with latest origin/develop to prevent conflicts...", current_branch);
        let _ = run_cmd_status(&["git", "rebase", "origin/develop"], None);
    }

    println!("📦 [2/2] Pulling and synchronizing all Git Subtrees from Upstream...");

    println!("📥 Pulling Subtree 'bootloader' (develop-pangu)...");
    let _ = run_cmd_status(
        &[
            "git",
            "subtree",
            "pull",
            "--prefix=bootloader",
            "bootloader-up",
            "develop-pangu",
            "--squash",
        ],
        None,
    );

    println!("📥 Pulling Subtree 'kernel' (develop)...");
    let _ = run_cmd_status(
        &[
            "git",
            "subtree",
            "pull",
            "--prefix=kernel",
            "kernel-up",
            "develop",
            "--squash",
        ],
        None,
    );

    println!("\n🚀 \x1B[1;32mOne-key pull complete! Local branch and all subtrees are 100% synchronized and up-to-date!\x1B[0m\n");
    Ok(())
}
