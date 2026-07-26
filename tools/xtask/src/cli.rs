use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "xtask")]
#[command(about = "CapsuleOS xtask Build System")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Clean all build artifacts (cargo clean + custom directories)")]
    Clean,
    #[command(about = "Operating System code development and execution (Build, Run, Check Env)")]
    Code {
        #[command(subcommand)]
        sub: CodeSubcommands,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum CodeSubcommands {
    #[command(about = "Build CapsuleOS kernel, standard library, and userspace programs")]
    Build {
        #[arg(
            long,
            default_value = "aarch64",
            help = "The target architecture (aarch64)"
        )]
        arch: String,
        #[arg(
            long,
            default_value = "virt",
            help = "The target platform profile (virt, rpi)"
        )]
        platform: String,
        #[arg(
            long,
            help = "Override runtime-config path (default: xtask.{platform}.toml)"
        )]
        config: Option<PathBuf>,
    },
    #[command(about = "Compile all architectures and launch CapsuleOS inside QEMU emulator")]
    Run {
        #[arg(
            long,
            default_value = "aarch64",
            help = "The target architecture (aarch64)"
        )]
        arch: String,
        #[arg(
            long,
            default_value = "virt",
            help = "The target platform profile (virt, rpi)"
        )]
        platform: String,
        #[arg(
            long,
            help = "Override runtime-config path (default: xtask.{platform}.toml)"
        )]
        config: Option<PathBuf>,
        #[arg(
            long,
            help = "Start QEMU in suspended state, listening on TCP port 1234 for GDB connection"
        )]
        gdb: bool,
    },
    #[command(
        about = "Verify if the local environment and cross-compilation toolchain are properly configured"
    )]
    CheckEnv {
        #[arg(long, help = "The expected Rust compiler version")]
        expected_rust: Option<String>,
    },
    #[command(about = "Verify and synchronize Cargo.toml and project workspace versions")]
    CheckVersion {
        #[arg(
            long,
            help = "Synchronize root workspace and xtask project version across subprojects"
        )]
        sync: bool,
    },
    #[command(about = "Generate unified and consolidated rust-doc website for all layers")]
    Doc {
        #[arg(
            long,
            help = "Automatically start a local server and open the documentation portal in your browser"
        )]
        open: bool,
    },
    #[command(
        about = "Boot CapsuleOS in QEMU, scrape `testall` output and report PASS/FAIL counts"
    )]
    Test {
        #[arg(
            long,
            default_value = "aarch64",
            help = "The target architecture (aarch64)"
        )]
        arch: String,
        #[arg(
            long,
            default_value = "virt",
            help = "The target platform profile (virt, rpi)"
        )]
        platform: String,
        #[arg(
            long,
            help = "Override runtime-config path (default: xtask.{platform}.toml)"
        )]
        config: Option<PathBuf>,
        #[arg(
            long,
            default_value_t = 90,
            help = "How long to wait for the `N/N passed` summary line"
        )]
        timeout: u64,
    },
}