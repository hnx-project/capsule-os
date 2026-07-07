use crate::repo::{load_config, run_cmd, run_cmd_status};

pub fn handle_setup_fork(username: &str) -> Result<(), String> {
    let config = load_config()?;
    println!("🔄 Setting up fork topology for user '{}'...", username);

    // 1. Setup Main Repository Remote
    let cur_origin = run_cmd(&["git", "remote", "get-url", "origin"], None)?;
    if cur_origin.contains(&config.gitcode.upstream_owner) {
        println!("🚀 Transitioning main repository remote 'origin' to 'upstream'...");
        run_cmd_status(&["git", "remote", "rename", "origin", "upstream"], None)?;
        let new_origin = format!(
            "git@gitcode.com:{}/{}.git",
            username, config.gitcode.upstream_repo
        );
        println!(
            "➕ Adding new personal fork remote 'origin' pointing to: {}",
            new_origin
        );
        run_cmd_status(&["git", "remote", "add", "origin", &new_origin], None)?;
    } else {
        println!(
            "ℹ️ Main repository origin is already set up or custom: {}",
            cur_origin
        );
    }

    // Fetch from newly defined remotes
    println!("📥 Fetching updates from origin & upstream...");
    let _ = run_cmd_status(&["git", "fetch", "origin"], None);
    let _ = run_cmd_status(&["git", "fetch", "upstream"], None);

    // Ensure we track origin/develop to eliminate visual push warnings in IDEs
    println!("⚙️ Binding local develop branch to origin/develop as tracker...");
    let _ = run_cmd_status(&["git", "branch", "--set-upstream-to=origin/develop"], None);

    // 2. Setup Subtrees Remotes
    println!("🚀 Setting up Subtree Upstream Remotes...");
    let _ = run_cmd_status(
        &[
            "git",
            "remote",
            "add",
            "bootloader-up",
            "git@gitcode.com:hnx-project/capsule-bootloader.git",
        ],
        None,
    );
    let _ = run_cmd_status(
        &[
            "git",
            "remote",
            "add",
            "kernel-up",
            "git@gitcode.com:hnx-project/hnx-core.git",
        ],
        None,
    );
    let _ = run_cmd_status(
        &[
            "git",
            "remote",
            "add",
            "ohlink-cc-up",
            "git@gitcode.com:hnx-project/ohlink-cc.git",
        ],
        None,
    );

    let _ = run_cmd_status(&["git", "fetch", "--all"], None);

    println!("🎉 Fork topology and Subtree remotes set up completed perfectly!");
    Ok(())
}
