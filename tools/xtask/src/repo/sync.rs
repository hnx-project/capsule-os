use crate::repo::{run_cmd, run_cmd_status};

pub fn handle_sync() -> Result<(), String> {
    println!("📥 Fetching upstream and updating submodules...");
    run_cmd_status(&["git", "fetch", "upstream"], None).map_err(|_| {
        "Could not fetch from upstream. Ensure you ran setup-fork first.".to_string()
    })?;

    println!("🔀 Rebasing local develop against upstream/develop...");
    let cur_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    if cur_branch == "develop" {
        run_cmd_status(&["git", "rebase", "upstream/develop"], None)?;
    } else {
        println!(
            "ℹ️ Active branch is '{}', skipped default rebase. You can manually rebase.",
            cur_branch
        );
    }

    println!("📦 Synchronizing Git submodules recursively...");
    run_cmd_status(
        &["git", "submodule", "update", "--init", "--recursive"],
        None,
    )
    .map_err(|_| "Failed to synchronize submodules recursively.".to_string())?;

    println!("✅ Worktree and submodules sync successfully synchronized!");
    Ok(())
}
