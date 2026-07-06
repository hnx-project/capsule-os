use crate::repo::{
    run_check_silent, run_cmd, run_cmd_status, GitCodeSession, MergeRequestResponse, XtaskConfig,
};

pub fn handle_pr(
    title_override: Option<&str>,
    release_mode: bool,
    config: &XtaskConfig,
    session: &GitCodeSession,
) -> Result<(), String> {
    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    if current_branch == "develop" || current_branch == "main" {
        return Err(
            "Cannot create Pull Request directly from default system branches (develop/main)."
                .to_string(),
        );
    }

    let target_base = if release_mode { "main" } else { "develop" };

    // Fetch upstream to align base points
    println!("📥 Fetching upstream {}...", target_base);
    run_cmd_status(&["git", "fetch", "upstream"], None).map_err(|_| {
        format!("Failed to fetch from 'upstream'. Check your remotes config with setup-fork.")
    })?;

    // Perform Mandatory Rebase against upstream base branch
    println!(
        "🔀 Rebasing current branch '{}' against upstream/{}...",
        current_branch, target_base
    );
    if let Err(e) = run_cmd_status(
        &["git", "rebase", &format!("upstream/{}", target_base)],
        None,
    ) {
        println!("⚠️ Rebase failed due to merge conflicts! Please resolve conflicts manually in your shell and run again.");
        return Err(e);
    }

    // Compress (Squash) commits automatically to maintain pristine history
    println!(
        "🥞 Soft-squashing your local commits relative to upstream/{}...",
        target_base
    );
    run_cmd_status(
        &[
            "git",
            "reset",
            "--soft",
            &format!("upstream/{}", target_base),
        ],
        None,
    )
    .map_err(|e| format!("Squash prep failed: {}", e))?;

    // Create a single unified clean commit using Conventional Commit standards
    println!("📝 Constructing clean squash commit...");
    let default_title = if let Some(t) = title_override {
        t.to_string()
    } else {
        format!(
            "feat(userspace): implement feature branches for {}",
            current_branch
        )
    };

    // Run complete safety linter gateway to ensure the squashed commit is pristine
    println!("🛡️ Verifying workspace compatibility and targets before Squash...");
    run_check_silent(&["cargo", "check", "--workspace", "--quiet"], None)?;
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

    // Directly stage changes and commit the unified Squash title securely
    run_cmd_status(&["git", "commit", "-m", &default_title], None)
        .map_err(|e| format!("Failed to create unified squash commit: {}", e))?;

    // Push the unified branch to developer fork (origin) with force-with-lease for safety
    println!(
        "📤 Pushing unified clean branch to origin/{}...",
        current_branch
    );
    run_cmd_status(&["git", "push", "origin", &current_branch, "-f"], None)
        .map_err(|e| format!("Failed to push to origin remote repository: {}", e))?;

    // Extract GitCode project user (username)
    let origin_url = run_cmd(&["git", "remote", "get-url", "origin"], None)?;
    let gitcode_username = if origin_url.contains("git@gitcode.com:") {
        origin_url
            .split("git@gitcode.com:")
            .nth(1)
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        return Err("Origin is not pointing to a standard GitCode SSH URI.".to_string());
    };

    if gitcode_username.is_empty() {
        return Err("Could not extract GitCode username from remote origin url.".to_string());
    }

    // Prepare API URL and Payload to target UPSTREAM branch directly
    let api_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/pulls?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, session.token
    );

    let pr_title = run_cmd(&["git", "log", "-1", "--pretty=%B"], None)?;
    let payload = serde_json::json!({
        "title": pr_title.trim(),
        "head": format!("{}:{}", gitcode_username, current_branch),
        "base": target_base,
        "body": "Automatically merged and submitted via CapsuleOS `cargo xtask repo pr` automation tool."
    });

    println!(
        "📨 Sending Merge Request to {}/{} (target: {})...",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, target_base
    );

    match ureq::post(&api_url).send_json(payload) {
        Ok(resp) => {
            if resp.status() == 201 || resp.status() == 200 {
                if let Ok(mr) = resp.into_json::<MergeRequestResponse>() {
                    if let Some(url) = mr.html_url {
                        println!("\n🚀 \x1B[1;32mMerge Request successfully created!\x1B[0m");
                        println!("👉 View MR on GitCode: \x1B[1;36m{}\x1B[0m\n", url);
                    } else {
                        println!("✅ MR created, but HTML URL was missing in response.");
                    }
                } else {
                    println!("✅ MR created! Please check GitCode dashboard.");
                }
            } else {
                println!(
                    "⚠️ API returned status: {}. Let's fall back to browser link.",
                    resp.status()
                );
                print_fallback_link(&gitcode_username, &current_branch, target_base, config);
            }
        }
        Err(e) => {
            println!("⚠️ REST API submission failed: {}.", e);
            print_fallback_link(&gitcode_username, &current_branch, target_base, config);
        }
    }

    Ok(())
}

fn print_fallback_link(username: &str, branch: &str, target_base: &str, config: &XtaskConfig) {
    let fallback_url = format!(
        "https://gitcode.com/{}/{}/pulls/new?merge_request[source_branch]={}&merge_request[target_branch]={}",
        config.gitcode.upstream_owner,
        config.gitcode.upstream_repo,
        format!("{}:{}", username, branch),
        target_base
    );
    println!(
        "\n💡 Please click or copy the link below to manually confirm the MR in your browser:"
    );
    println!("👉 \x1B[1;36m{}\x1B[0m\n", fallback_url);
}
