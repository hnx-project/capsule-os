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
}
