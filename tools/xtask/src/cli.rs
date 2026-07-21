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
}
