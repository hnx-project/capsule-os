use crate::cli::RepoSubcommands;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use zip::write::FileOptions;
use zip::ZipWriter;

#[derive(Deserialize, Debug, Clone)]
pub struct XtaskConfig {
    pub project: ProjectConfig,
    pub gitcode: GitCodeConfig,
    pub submodules: HashMap<String, SubmoduleConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub codename: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GitCodeConfig {
    pub upstream_owner: String,
    pub upstream_repo: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SubmoduleConfig {
    pub upstream: String,
    pub fork_repo: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GitCodeUser {
    pub login: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MergeRequestResponse {
    pub html_url: Option<String>,
    pub id: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GitCodePermissionResponse {
    pub permission: Option<String>,
}

pub struct GitCodeSession {
    pub token: String,
    pub username: String,
    pub permission: String,
    pub is_admin: bool,
}

pub fn load_config() -> Result<XtaskConfig, String> {
    let content = fs::read_to_string("xtask.toml")
        .map_err(|e| format!("Failed to read xtask.toml config: {}", e))?;
    let config: XtaskConfig =
        toml::from_str(&content).map_err(|e| format!("Failed to parse xtask.toml: {}", e))?;
    Ok(config)
}

pub fn run_cmd(args: &[&str], dir: Option<&Path>) -> Result<String, String> {
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

pub fn run_cmd_status(args: &[&str], dir: Option<&Path>) -> Result<(), String> {
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

pub fn run_check_silent(args: &[&str], dir: Option<&Path>) -> Result<(), String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
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

pub fn is_admin_mode(config: &XtaskConfig) -> bool {
    if let Ok(origin_url) = run_cmd(&["git", "remote", "get-url", "origin"], None) {
        let pattern = format!(
            "{}/{}",
            config.gitcode.upstream_owner, config.gitcode.upstream_repo
        );
        if origin_url.contains(&pattern) {
            return true;
        }
    }
    if let Ok(remotes) = run_cmd(&["git", "remote"], None) {
        if !remotes.contains("upstream") {
            return true;
        }
    }
    false
}

pub fn verify_branch_name(branch: &str) -> Result<(), String> {
    if branch == "develop" || branch == "main" {
        return Ok(());
    }
    let pattern = r"^(feat|fix|chore|style|refactor|perf|test|docs)/[a-zA-Z0-9\-_]+$";
    let re = Regex::new(pattern).unwrap();
    if !re.is_match(branch) {
        return Err(format!(
            "❌ Invalid Branch Name: '{}'!\n\
             All development branch names MUST conform to the Conventional Commits directory structure:\n\
             - Format: <type>/<description> (e.g. feat/add-loader, fix/process-leak, chore/update-deps)\n\
             - Standard types: feat, fix, chore, style, refactor, perf, test, docs.",
            branch
        ));
    }
    Ok(())
}

pub fn preflight_check(config: &XtaskConfig) -> Result<GitCodeSession, String> {
    if is_admin_mode(config) {
        return Ok(GitCodeSession {
            token: "admin_token".to_string(),
            username: "admin".to_string(),
            permission: "admin".to_string(),
            is_admin: true,
        });
    }

    println!("🔎 Running preflight GitCode Token check...");
    let token = if let Ok(pat) = env::var("GITCODE_PAT") {
        pat
    } else {
        // Fallback to local config file ~/.config/gitcode/token using standard HOME env
        let home = env::var("HOME").map_err(|_| {
            "❌ Could not determine user HOME directory to find GitCode Token.".to_string()
        })?;
        let mut home_dir = PathBuf::from(home);
        home_dir.push(".config/gitcode/token");
        if home_dir.exists() {
            fs::read_to_string(&home_dir)
                .map_err(|e| format!("Failed to read local GitCode token file: {}", e))?
                .trim()
                .to_string()
        } else {
            return Err("❌ Missing GitCode Personal Access Token!\n\
                       Please set GITCODE_PAT environment variable or write your token inside ~/.config/gitcode/token to authenticate."
                .to_string());
        }
    };

    let check_user_url = format!("https://gitcode.com/api/v5/user?access_token={}", token);
    let resp = ureq::get(&check_user_url)
        .send_string("")
        .map_err(|e| format!("❌ GITCODE_PAT is invalid or network error: {}", e))?;

    let user_info: GitCodeUser = resp
        .into_json()
        .map_err(|e| format!("❌ Failed to parse user response from GitCode: {}", e))?;

    let permission_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/collaborators/{}/permission?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, user_info.login, token
    );

    let mut permission = "none".to_string();
    if let Ok(p_resp) = ureq::get(&permission_url).send_string("") {
        if let Ok(p_info) = p_resp.into_json::<GitCodePermissionResponse>() {
            if let Some(perm) = p_info.permission {
                permission = perm;
            }
        }
    }

    let is_admin = permission == "admin" || permission == "owner";

    Ok(GitCodeSession {
        token,
        username: user_info.login,
        permission,
        is_admin,
    })
}

// Submodule handlers will be integrated under handle_repo
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
            let session = preflight_check(&config)?;
            handle_pr(title.as_deref(), *release, &config, &session)?;
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
        RepoSubcommands::Push => {
            handle_push(&config)?;
        }
    }
    Ok(())
}

// Declare externalized submodules inside repo folder
pub mod add;
pub mod commit;
pub mod pr;
pub mod pull;
pub mod push;
pub mod release;
pub mod setup_fork;
pub mod sync;

use add::handle_add;
use commit::handle_commit;
use pr::handle_pr;
use pull::handle_pull;
use push::handle_push;
use release::handle_release;
use setup_fork::handle_setup_fork;
use sync::handle_sync;
