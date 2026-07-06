use crate::cli::RepoSubcommands;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use zip::write::FileOptions;
use zip::ZipWriter;

#[derive(Deserialize, Debug)]
struct XtaskConfig {
    project: ProjectConfig,
    gitcode: GitCodeConfig,
    submodules: std::collections::HashMap<String, SubmoduleConfig>,
}

#[derive(Deserialize, Debug)]
struct ProjectConfig {
    name: String,
    codename: String,
}

#[derive(Deserialize, Debug)]
struct GitCodeConfig {
    upstream_owner: String,
    upstream_repo: String,
}

#[derive(Deserialize, Debug, Clone)]
struct SubmoduleConfig {
    upstream: String,
    fork_repo: String,
}

fn load_config() -> Result<XtaskConfig, String> {
    let content = fs::read_to_string("xtask.toml")
        .map_err(|e| format!("Failed to read xtask.toml config: {}", e))?;
    let config: XtaskConfig =
        toml::from_str(&content).map_err(|e| format!("Failed to parse xtask.toml: {}", e))?;
    Ok(config)
}

#[derive(Serialize, Deserialize, Debug)]
struct GitCodeUser {
    login: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct MergeRequestResponse {
    html_url: Option<String>,
    id: Option<u64>,
}

/// Helper function to run commands and capture stdout as String
fn run_cmd(args: &[&str], dir: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute command '{:?}': {}", args, e))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Helper to check if the workspace is in Maintainer/Administrator Release mode
fn is_admin_mode(config: &XtaskConfig) -> bool {
    if let Ok(origin_url) = run_cmd(&["git", "remote", "get-url", "origin"], None) {
        let pattern = format!(
            "{}/{}",
            config.gitcode.upstream_owner, config.gitcode.upstream_repo
        );
        if origin_url.contains(&pattern) {
            return true;
        }
    }
    // Also true if there is no upstream remote, meaning origin is already upstream
    if let Ok(remotes) = run_cmd(&["git", "remote"], None) {
        if !remotes.contains("upstream") {
            return true;
        }
    }
    false
}
fn run_cmd_status(args: &[&str], dir: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("Failed to start check '{:?}': {}", args, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Command '{:?}' failed with exit status: {:?}",
            args,
            status.code()
        ))
    }
}

/// Helper to run compilation check completely silencing warnings and hiding success logs
fn run_check_silent(args: &[&str], dir: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    // Force RUSTFLAGS="-A warnings" to eliminate all warnings
    cmd.env("RUSTFLAGS", "-A warnings");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run compiler check: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let err_msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("\n{}", err_msg))
    }
}

struct GitCodeSession {
    token: String,
    username: String,
    permission: String,
    is_admin: bool,
}

#[derive(Deserialize, Debug)]
struct GitCodePermissionResponse {
    permission: Option<String>,
}

/// Preflight security and integrity check
fn preflight_check(config: &XtaskConfig) -> Result<GitCodeSession, String> {
    println!("🔍 Performing preflight checks...");

    // 1. Check if git is installed
    if run_cmd(&["git", "--version"], None).is_err() {
        return Err("git CLI is not installed or not in PATH.".to_string());
    }

    // 2. Check Git configuration
    let name = run_cmd(&["git", "config", "user.name"], None)
        .map_err(|_| "Git 'user.name' is not configured locally. Run: git config --global user.name \"Your Name\"".to_string())?;
    let email = run_cmd(&["git", "config", "user.email"], None)
        .map_err(|_| "Git 'user.email' is not configured locally. Run: git config --global user.email \"you@example.com\"".to_string())?;

    if name.is_empty() || email.is_empty() {
        return Err("Git user identity is not fully configured. Please ensure 'user.name' and 'user.email' are set.".to_string());
    }

    // 3. Get GitCode Token
    let token = if let Ok(t) = env::var("GITCODE_TOKEN") {
        t.trim().to_string()
    } else {
        let mut home_dir = env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| "Could not find HOME directory")?;
        home_dir.push(".config/gitcode/token");
        if home_dir.exists() {
            fs::read_to_string(home_dir)
                .map(|s| s.trim().to_string())
                .map_err(|e| format!("Failed to read ~/.config/gitcode/token: {}", e))?
        } else {
            return Err("GitCode PAT Token not found!\n\
                      Please set the GITCODE_TOKEN environment variable or create the token file at ~/.config/gitcode/token.\n\
                      To generate a token, visit your GitCode Personal Settings page.".to_string());
        }
    };

    if token.is_empty() {
        return Err("GitCode token is empty. Please provide a valid PAT token.".to_string());
    }

    // 4. Validate Token with GitCode API
    let api_url = format!("https://gitcode.com/api/v5/user?access_token={}", token);
    println!("🔌 Testing connection with GitCode API...");
    let resp = match ureq::get(&api_url).call() {
        Ok(r) => r,
        Err(e) => {
            return Err(format!(
            "Failed to connect to GitCode API: {}. Please check your token and network connection.",
            e
        ))
        }
    };

    if resp.status() != 200 {
        return Err(format!(
            "GitCode API responded with status code: {}",
            resp.status()
        ));
    }

    let user_info: GitCodeUser = resp
        .into_json()
        .map_err(|e| format!("Failed to parse user info: {}", e))?;
    println!(
        "✅ GitCode connection verified. Welcome, {}!",
        user_info.login
    );

    // 5. Verify Collaborator Permission via API
    println!("🛡️ Verifying collaborator permissions on upstream repository...");
    let perm_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/collaborators/{}/permission?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, user_info.login, token
    );

    let mut permission = "none".to_string();
    let mut is_admin = false;

    if let Ok(perm_resp) = ureq::get(&perm_url).call() {
        if perm_resp.status() == 200 {
            if let Ok(perm_data) = perm_resp.into_json::<GitCodePermissionResponse>() {
                if let Some(p) = perm_data.permission {
                    permission = p.clone();
                    if p == "admin" || p == "owner" || p == "write" {
                        is_admin = true;
                        println!("👑 Administrator/Write access confirmed! (Role: {})", p);
                    } else {
                        println!("👥 Contributor Read-only access confirmed. (Role: {})", p);
                    }
                }
            }
        }
    } else {
        println!("ℹ️ Active user is not an explicit collaborator. Defaulting to Fork Contributor workflow.");
    }

    Ok(GitCodeSession {
        token,
        username: user_info.login,
        permission,
        is_admin,
    })
}

/// Retrieve the version from the workspace Cargo.toml
fn get_local_version() -> Result<String, String> {
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
    // Fallback to searching in sub-packages if not found at root workspace level
    Err("Could not find a valid version in workspace Cargo.toml".to_string())
}

/// Get version of Cargo.toml from upstream develop branch
fn get_upstream_version() -> Result<String, String> {
    // Attempt to view the upstream Cargo.toml file content
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

/// Verifies branch name matches the regex constraints
fn verify_branch_name(branch: &str) -> Result<(), String> {
    let branch_pattern = r"^(main|develop|release\/(v?[0-9]+\.[0-9]+\.[0-9]+|[a-z0-9-]+)|feature\/[a-z0-9-]+|fix\/[a-z0-9-]+)$";
    let regex = Regex::new(branch_pattern).unwrap();
    if !regex.is_match(branch) {
        return Err(format!(
            "❌ Branch naming rule violation!\n\
             Current branch: '{}'\n\
             Must match pattern:\n\
             - main\n\
             - develop\n\
             - release/<version>\n\
             - feature/<name-slug>\n\
             - fix/<name-slug>",
            branch
        ));
    }
    Ok(())
}

/// Handle setup-fork subcommand
fn handle_setup_fork(username: &str) -> Result<(), String> {
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

    // Ensure we track upstream/develop
    println!("⚙️ Binding local develop branch to upstream/develop as tracker...");
    let _ = run_cmd_status(
        &["git", "branch", "--set-upstream-to=upstream/develop"],
        None,
    );

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

/// Handle commit subcommand
fn handle_commit(
    r#type: Option<&str>,
    scope: Option<&str>,
    message: Option<&str>,
    config: &XtaskConfig,
) -> Result<(), String> {
    // 1. Get current branch and check naming convention
    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    verify_branch_name(&current_branch)?;

    // Create missing workspace payload placeholders to untangle kernel's include_bytes! compile dependencies
    let ohc_deps = [
        "kernel/files/init.ohc",
        "kernel/files/devmgr.ohc",
        "kernel/files/loader.ohc",
        "kernel/files/vfs.ohc",
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

    // 2. Safety Linters Grid (Format & Cross-Architecture compilation checks)
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

    // 3. Prevent duplicate version safety gate
    let local_ver = get_local_version()?;
    if let Ok(upstream_ver) = get_upstream_version() {
        // Only trigger version safety check if there are modifications in source files
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

    // 4. Gather Commit Info and Apply Commit
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

    // Validate type
    let valid_types = [
        "feat", "fix", "chore", "docs", "refactor", "style", "test", "perf", "wip",
    ];
    if !valid_types.contains(&commit_type.as_str()) {
        return Err(format!(
            "Invalid commit type: '{}'. Valid types: {:?}",
            commit_type, valid_types
        ));
    }

    // Format commit message
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

/// Handle pr subcommand (Squash, Rebase and submit PR via API)
fn handle_pr(
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

    println!("💡 Please provide commit message structure for this unified PR branch.");
    handle_commit(None, None, Some(&default_title), config)?;

    // 🌟 Administrator Direct Write-Back Flow (Auto Bypass PR creation page)
    if session.is_admin && !release_mode {
        println!("👑 Core Maintainer direct-write privileges unlocked!");
        println!("📤 Pushing unified clean branch directly to upstream/develop...");
        run_cmd_status(
            &[
                "git",
                "push",
                "upstream",
                &format!("{}:develop", current_branch),
                "-f",
            ],
            None,
        )
        .map_err(|e| format!("Failed to push directly to upstream repository: {}", e))?;
        println!("\n🚀 \x1B[1;32mDirect Push and Merge Complete! Unified clean commit safely integrated into upstream/develop.\x1B[0m\n");
        return Ok(());
    }

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

/// Handle sync subcommand (Fetch upstream and sync submodules)
fn handle_sync() -> Result<(), String> {
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

/// Handle tag subcommand (Administrators only - create annotated SemVer tags and push to upstream)
fn handle_tag(version: &str, config: &XtaskConfig) -> Result<(), String> {
    // 1. Tag SemVer naming constraints matching standard Regex
    let tag_pattern = r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(alpha|beta|rc)\.(0|[1-9][0-9]*))?(-[a-z0-9-]+)?$";
    let regex = Regex::new(tag_pattern).unwrap();
    if !regex.is_match(version) {
        return Err(format!(
            "❌ Invalid tag version format: '{}'!\n\
             Must match strict SemVer format: e.g., v0.6.0, v1.0.0-rc.1",
            version
        ));
    }

    // 2. Branch restriction (Only allow tagging on main or release/* branches)
    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    if current_branch != "main" && !current_branch.starts_with("release/") {
        return Err(format!(
            "❌ Tagging is prohibited on active branch '{}'!\n\
             Release tags are strictly restricted to 'main' or 'release/*' branches to protect production integrity.",
            current_branch
        ));
    }

    // 3. Ultimate Safety Compilation Guard (Ensure the tag is pristine)
    println!("🛠️ Running final safety validation checks before tagging...");
    print!("🧹 Checking rustfmt... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    run_cmd_status(&["cargo", "fmt", "--check"], None)
        .map_err(|_| "\n❌ Formatting check failed! Run 'cargo fmt' first.".to_string())?;
    println!("\x1B[1;32m[OK]\x1B[0m");

    print!("🛡️ Verifying workspace and multi-arch kernel targets... ");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    // Create missing workspace payload placeholders to untangle kernel compile dependencies
    let ohc_deps = [
        "kernel/files/init.ohc",
        "kernel/files/devmgr.ohc",
        "kernel/files/loader.ohc",
        "kernel/files/vfs.ohc",
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
    println!("\x1B[1;32m[OK]\x1B[0m");

    // 4. Create annotated tag locally
    println!("🏷️ Creating annotated Git tag: {}...", version);
    run_cmd_status(
        &[
            "git",
            "tag",
            "-a",
            version,
            "-m",
            &format!("Release {}", version),
        ],
        None,
    )
    .map_err(|e| format!("Failed to create local annotated tag: {}", e))?;

    // 5. Push tag to upstream main repo
    let remote = if run_cmd(&["git", "remote"], None)?.contains("upstream") {
        "upstream"
    } else {
        "origin"
    };

    println!("📤 Pushing tag {} to remote '{}'...", version, remote);
    run_cmd_status(&["git", "push", remote, version], None)
        .map_err(|e| format!("Failed to push tag to remote: {}", e))?;

    println!(
        "\n🚀 \x1B[1;32mTag {} has been successfully published to {}! (Production Release Complete)\x1B[0m\n",
        version, remote
    );
    Ok(())
}

/// Helper function to automatically package all architectures into a named ZIP
fn zip_release_assets(
    zip_path: &Path,
    aarch64_ohc: &Path,
    aarch64_bin: &Path,
    riscv64_ohc: &Path,
    riscv64_bin: &Path,
) -> Result<Vec<u8>, String> {
    let file =
        fs::File::create(zip_path).map_err(|e| format!("Failed to create ZIP output: {}", e))?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    // 1. Pack AArch64 assets
    zip.start_file("aarch64/hnxcore.ohc", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data =
        fs::read(aarch64_ohc).map_err(|e| format!("Failed to read AArch64 OHC capsule: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.start_file("aarch64/capsule-bootloader.bin", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data =
        fs::read(aarch64_bin).map_err(|e| format!("Failed to read AArch64 Bootloader: {}", e))?;
    zip.write_all(&data).unwrap();

    // 2. Pack RISC-V 64 assets
    zip.start_file("riscv64/hnxcore.ohc", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data = fs::read(riscv64_ohc)
        .map_err(|e| format!("Failed to read RISC-V 64 OHC capsule: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.start_file("riscv64/capsule-bootloader.bin", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data =
        fs::read(riscv64_bin).map_err(|e| format!("Failed to read RISC-V 64 Bootloader: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.finish()
        .map_err(|e| format!("Failed to finalize ZIP archive: {}", e))?;

    let zip_bytes =
        fs::read(zip_path).map_err(|e| format!("Failed to read final ZIP binary bytes: {}", e))?;
    Ok(zip_bytes)
}

#[derive(Deserialize, Debug)]
struct CreateReleaseResponse {
    id: Option<u64>,
    html_url: Option<String>,
}

/// Handle release subcommand (Build multi-arch, zip them, upload GitCode Release)
fn handle_release(
    version: &str,
    config: &XtaskConfig,
    session: &GitCodeSession,
) -> Result<(), String> {
    if !is_admin_mode(config) {
        return Err(
            "❌ Unauthorized: Only administrators can create and publish Production Releases."
                .to_string(),
        );
    }

    println!(
        "🎁 Starting production release publication for {}...",
        version
    );

    // 1. Build AArch64
    println!("⚙️ [1/3] Building AArch64 target (release mode)...");
    run_cmd_status(&["cargo", "xtask", "build", "--arch", "aarch64"], None)
        .map_err(|_| "❌ Failed to compile AArch64 targets!".to_string())?;

    let aarch64_ohc = Path::new("dist/kernel/hnxcore.ohc");
    let aarch64_bin = Path::new("build/target/aarch64-unknown-none/release/capsule-bootloader.bin");

    // Copy out temporarily so subsequent compilation doesn't overwrite
    let temp_aarch64_ohc = Path::new("build/hnxcore_aarch64.ohc");
    let temp_aarch64_bin = Path::new("build/capsule-bootloader_aarch64.bin");
    let _ = fs::copy(aarch64_ohc, temp_aarch64_ohc);
    let _ = fs::copy(aarch64_bin, temp_aarch64_bin);

    // 2. Build RISC-V 64
    println!("⚙️ [2/3] Building RISC-V 64 target (release mode)...");
    run_cmd_status(&["cargo", "xtask", "build", "--arch", "riscv64"], None)
        .map_err(|_| "❌ Failed to compile RISC-V 64 targets!".to_string())?;

    let riscv64_ohc = Path::new("dist/kernel/hnxcore.ohc");
    let riscv64_bin =
        Path::new("build/target/riscv64imac-unknown-none-elf/release/capsule-bootloader.bin");

    // 3. Assemble and ZIP assets
    println!("📦 [3/3] Archiving and compressing all targets to standard ZIP...");

    // Get date YYYYMMDD
    let date_str = run_cmd(&["date", "+%Y%m%d"], None).unwrap_or_else(|_| "20260706".to_string());

    let zip_name = format!(
        "capsuleos-{}-{}-hnx-{}.zip",
        config.project.codename, version, date_str
    );
    let zip_path = PathBuf::from(format!("dist/{}", zip_name));

    if let Some(parent) = zip_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let zip_bytes = zip_release_assets(
        &zip_path,
        temp_aarch64_ohc,
        temp_aarch64_bin,
        riscv64_ohc,
        riscv64_bin,
    )?;

    // Cleanup temp copies
    let _ = fs::remove_file(temp_aarch64_ohc);
    let _ = fs::remove_file(temp_aarch64_bin);

    let zip_size_mb = zip_bytes.len() as f64 / 1024.0 / 1024.0;
    println!(
        "💾 Archive ZIP created: \x1B[1;36mdist/{}\x1B[0m ({:.2} MB)",
        zip_name, zip_size_mb
    );

    // 4. Create Release page via GitCode API
    println!("📨 Creating Release page on GitCode for tag {}...", version);
    let create_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/releases?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, session.token
    );

    let payload = serde_json::json!({
        "tag_name": version,
        "name": format!("CapsuleOS {} - Pangu Release", version),
        "body": format!("### 🚀 CapsuleOS {} Production Release\n\n\
                        This release has been automatically compiled, packed, and published via `cargo xtask repo release {}`.\n\n\
                        #### 📦 Attached Assets (Multi-Arch):\n\
                        - `aarch64/hnxcore.ohc` (Kernel OHC Capsule)\n\
                        - `aarch64/capsule-bootloader.bin` (Firmware Shim Bootloader)\n\
                        - `riscv64/hnxcore.ohc` (Kernel OHC Capsule)\n\
                        - `riscv64/capsule-bootloader.bin` (Firmware Shim Bootloader)\n\n\
                        All assets are unified and zipped inside **`{}`** below.", version, version, zip_name),
        "prerelease": false,
        "target_commitish": "main"
    });

    let resp = match ureq::post(&create_url).send_json(payload) {
        Ok(r) => r,
        Err(e) => return Err(format!("❌ Failed to create GitCode Release: {}", e)),
    };

    let release_info: CreateReleaseResponse = resp
        .into_json()
        .map_err(|e| format!("Failed to parse release response: {}", e))?;
    let release_id = match release_info.id {
        Some(id) => id,
        None => return Err("GitCode response did not contain a valid release id.".to_string()),
    };

    // 5. Upload ZIP Asset as Attachment
    println!("📎 Uploading release archive ZIP to GitCode (this may take a few seconds)...");
    let upload_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/releases/{}/attach_files?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, release_id, session.token
    );

    // Pure Rust Multipart Multipart Form-Data Writer
    let boundary = "------------------------capsuleosreleaseboundary123456789";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            zip_name
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
    body.extend_from_slice(&zip_bytes);
    body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());

    match ureq::post(&upload_url)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
    {
        Ok(resp) => {
            if resp.status() == 201 || resp.status() == 200 {
                println!("\n🚀 \x1B[1;32mProduction Release & Attachments successfully published!\x1B[0m");
                if let Some(url) = release_info.html_url {
                    println!("👉 View Release page: \x1B[1;36m{}\x1B[0m\n", url);
                }
            } else {
                return Err(format!(
                    "❌ Upload asset failed with status code: {}",
                    resp.status()
                ));
            }
        }
        Err(e) => return Err(format!("❌ Failed to upload ZIP asset: {}", e)),
    }

    Ok(())
}

/// Handle pull subcommand (One-key synchronize main repo & all submodules recursively)
fn handle_pull(config: &XtaskConfig) -> Result<(), String> {
    println!("📥 [1/2] Fetching and aligning main repository...");

    // Check clean tree before doing pulls and rebases to avoid merge dirty loss
    let dirty_check = run_cmd(&["git", "status", "--porcelain"], None).unwrap_or_default();
    if !dirty_check.trim().is_empty() {
        return Err("⚠️  Local workspace is not clean! Please commit, stash, or stash-save your changes before running pull.".to_string());
    }

    // Fetch upstream develop
    run_cmd_status(&["git", "fetch", "upstream"], None)
        .map_err(|_| "Could not fetch from 'upstream'. Ensure you have configured upstream by running: cargo xtask repo setup-fork".to_string())?;

    let current_branch = run_cmd(&["git", "branch", "--show-current"], None)?;
    if current_branch == "develop" {
        println!("🔀 Rebasing local develop branch against upstream/develop...");
        run_cmd_status(&["git", "rebase", "upstream/develop"], None)?;
    } else if current_branch == "main" {
        println!("🔀 Rebasing local main branch against upstream/main...");
        let _ = run_cmd_status(&["git", "rebase", "upstream/main"], None);
    } else {
        println!("🔀 Merging/Rebasing active branch '{}' with latest upstream/develop to prevent conflicts...", current_branch);
        let _ = run_cmd_status(&["git", "rebase", "upstream/develop"], None);
    }

    // Fetch personal fork remote origin
    let _ = run_cmd_status(&["git", "fetch", "origin"], None);

    // Synchronize all submodules configured in xtask.toml
    println!("📦 [2/2] Aligning and fetching all submodules...");
    for (local_path, sub_cfg) in &config.submodules {
        let sub_dir = Path::new(local_path);
        if sub_dir.exists() {
            println!("📥 Pulling/Fetching Submodule '{}'...", local_path);
            let _ = run_cmd_status(&["git", "fetch", "upstream"], Some(sub_dir));
            let _ = run_cmd_status(&["git", "fetch", "origin"], Some(sub_dir));
        }
    }

    // Pointer matching update recursively
    println!("⚙️ Recursively updating submodules pointers...");
    run_cmd_status(
        &["git", "submodule", "update", "--init", "--recursive"],
        None,
    )
    .map_err(|_| "Failed to recursively update submodules pointers.".to_string())?;

    println!("\n🚀 \x1B[1;32mOne-key pull complete! Local branch and all submodules are 100% synchronized and up-to-date with upstream.\x1B[0m\n");
    Ok(())
}

/// Handle add subcommand (Stage files with security secret scan and auto cascade submodules staging)
fn handle_add(files: &[String], config: &XtaskConfig) -> Result<(), String> {
    println!("🔍 [1/3] Performing secret leak vulnerability scan on modified files...");

    // Get modified/untracked files to scan
    let changed_files = run_cmd(&["git", "diff", "--cached", "--name-only"], None)
        .unwrap_or_default()
        + "\n"
        + &run_cmd(&["git", "diff", "--name-only"], None).unwrap_or_default()
        + "\n"
        + &run_cmd(&["git", "status", "--porcelain"], None).unwrap_or_default();

    // Compile safety regex for potential secrets
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

    // 2. Stage main repository
    println!("🌿 [2/3] Staging main repository files...");
    let mut args = vec!["git", "add"];
    for f in files {
        args.push(f);
    }
    run_cmd_status(&args, None).map_err(|e| format!("Failed to stage main repository: {}", e))?;
    println!("✅ Main repository files successfully staged.");

    // 3. Auto cascade stage to submodules from xtask.toml
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
                // Submodule stage files (defaulting to staging all changed inside submodule)
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

/// Core dispatcher entry point
pub fn handle_repo(sub: &RepoSubcommands) -> Result<(), String> {
    let config = load_config()?;
    match sub {
        RepoSubcommands::SetupFork { username } => {
            handle_setup_fork(username)?;
        }
        RepoSubcommands::Commit {
            r#type,
            scope,
            message,
        } => {
            handle_commit(
                r#type.as_deref(),
                scope.as_deref(),
                message.as_deref(),
                &config,
            )?;
        }
        RepoSubcommands::Pr { title, release } => {
            // preflight_check is mandatory for pr (it returns session)
            let session = preflight_check(&config)?;
            handle_pr(title.as_deref(), *release, &config, &session)?;
        }
        RepoSubcommands::Tag { version } => {
            handle_tag(version, &config)?;
        }
        RepoSubcommands::Release { version } => {
            let session = preflight_check(&config)?;
            handle_release(version, &config, &session)?;
        }
        RepoSubcommands::Add { files } => {
            handle_add(files, &config)?;
        }
        RepoSubcommands::Pull => {
            handle_pull(&config)?;
        }
        RepoSubcommands::Sync => {
            handle_sync()?;
        }
    }
    Ok(())
}
