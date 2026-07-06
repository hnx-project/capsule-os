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
    #[command(
        about = "Repository and collaboration workflow (Fork, Commit, PR, Sync, Pull, Release, Add)"
    )]
    Repo {
        #[command(subcommand)]
        sub: RepoSubcommands,
    },
    #[command(about = "Operating System development and execution (Build, Run, Check Env)")]
    Os {
        #[command(subcommand)]
        sub: OsSubcommands,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum OsSubcommands {
    #[command(about = "Build CapsuleOS kernel, standard library, and userspace programs")]
    Build {
        #[arg(
            long,
            default_value = "aarch64",
            help = "The target architecture (aarch64 / riscv64)"
        )]
        arch: String,
    },
    #[command(about = "Compile all architectures and launch CapsuleOS inside QEMU emulator")]
    Run {
        #[arg(
            long,
            default_value = "aarch64",
            help = "The target architecture (aarch64 / riscv64)"
        )]
        arch: String,
    },
    #[command(
        about = "Verify if the local environment and cross-compilation toolchain are properly configured"
    )]
    CheckEnv {
        #[arg(long, help = "The expected Rust compiler version")]
        expected_rust: Option<String>,
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
    #[command(
        about = "Safely validate code (run checks) and push current branch directly to your developer fork (origin)"
    )]
    Push,
}
