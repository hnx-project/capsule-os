use crate::repo::{run_cmd, run_cmd_status};

pub fn handle_sync() -> Result<(), String> {
    println!("📥 Fetching upstream and updating subtrees...");
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

    println!("📦 Synchronizing Git subtrees recursively from upstream...");
    println!("📥 Updating Subtree 'bootloader' (develop-pangu)...");
    let _ = run_cmd_status(&["git", "subtree", "pull", "--prefix=bootloader", "bootloader-up", "develop-pangu", "--squash"], None);

    println!("📥 Updating Subtree 'kernel' (develop)...");
    let _ = run_cmd_status(&["git", "subtree", "pull", "--prefix=kernel", "kernel-up", "develop", "--squash"], None);

    println!("📥 Updating Subtree 'tools/ohlink-cc' (main)...");
    let _ = run_cmd_status(&["git", "subtree", "pull", "--prefix=tools/ohlink-cc", "ohlink-cc-up", "main", "--squash"], None);

    println!("✅ Worktree and subtrees successfully synchronized!");
    Ok(())
}
