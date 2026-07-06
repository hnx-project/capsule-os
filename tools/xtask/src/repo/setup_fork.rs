use crate::repo::{load_config, run_cmd, run_cmd_status};
use std::path::Path;

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

    // 2. Setup Submodules dynamically from xtask.toml config
    for (local_path, sub_cfg) in &config.submodules {
        let sub_dir = Path::new(local_path);
        if sub_dir.exists() {
            println!("🚀 Transitioning '{}' submodule remote...", local_path);
            let sub_origin =
                run_cmd(&["git", "remote", "get-url", "origin"], Some(sub_dir)).unwrap_or_default();
            if sub_origin.contains(&sub_cfg.upstream)
                || sub_origin.contains(&config.gitcode.upstream_owner)
            {
                let _ = run_cmd_status(
                    &["git", "remote", "rename", "origin", "upstream"],
                    Some(sub_dir),
                );
                let sub_new_origin =
                    format!("git@gitcode.com:{}/{}.git", username, sub_cfg.fork_repo);
                let _ = run_cmd_status(
                    &["git", "remote", "add", "origin", &sub_new_origin],
                    Some(sub_dir),
                );
                println!("✅ Added {} fork: {}", local_path, sub_new_origin);

                // Ensure submodules track origin/develop to eliminate visual push warnings
                let _ = run_cmd_status(
                    &["git", "branch", "--set-upstream-to=origin/develop"],
                    Some(sub_dir),
                );
            } else {
                println!(
                    "ℹ️ Submodule '{}' remote already customized: {}",
                    local_path, sub_origin
                );
            }
        }
    }

    println!("🎉 Fork topology set up completed perfectly!");
    Ok(())
}
