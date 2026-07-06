use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "xtask")]
#[command(about = "CapsuleOS xtask Build System")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Build {
        #[arg(long, default_value = "aarch64")]
        arch: String,
    },
    Run {
        #[arg(long, default_value = "aarch64")]
        arch: String,
    },
    CheckToolchain {
        #[arg(long)]
        expected_rust: Option<String>,
    },
    #[command(about = "Repository workflow automation (setup-fork, commit, PR, sync)")]
    Repo {
        #[command(subcommand)]
        sub: RepoSubcommands,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum RepoSubcommands {
    #[command(about = "Set up fork topology for main repo and submodules")]
    SetupFork {
        #[arg(long, default_value = "TAPPI")]
        username: String,
    },
    #[command(
        about = "Safely check constraints and commit changes with Conventional Commit format"
    )]
    Commit {
        #[arg(
            short,
            long,
            help = "Commit type (e.g., feat, fix, chore, style, refactor, perf, test, docs)"
        )]
        r#type: Option<String>,
        #[arg(short, long, help = "Optional scope (e.g., kernel, std, bootloader)")]
        scope: Option<String>,
        #[arg(short, long, help = "Commit description message")]
        message: Option<String>,
    },
    #[command(
        about = "Fetch upstream develop, rebase, squash, push to fork and create a GitCode Merge Request"
    )]
    Pr {
        #[arg(
            short,
            long,
            help = "Optional MR title (defaults to your commit message)"
        )]
        title: Option<String>,
        #[arg(
            long,
            help = "Submit Merge Request targeting 'main' branch directly (Admin/Release mode)"
        )]
        release: bool,
    },
    #[command(
        about = "Build all architectures, package into ZIP, and publish GitCode Release (GitCode will auto-generate the tag upstream)"
    )]
    Release {
        #[arg(help = "The semver release version (e.g. v0.6.0) to publish")]
        version: String,
    },
    #[command(
        about = "Safely stage changed files, automatically scanning for secrets and cascade staging submodules"
    )]
    Add {
        #[arg(
            help = "Files to stage (defaults to '.' for all changed files)",
            default_value = "."
        )]
        files: Vec<String>,
    },
    #[command(
        about = "Pull and synchronize local branch and all submodules with upstream, automatically rebasing"
    )]
    Pull,
    #[command(about = "Safely synchronize your branch and submodules with upstream develop")]
    Sync,
}
