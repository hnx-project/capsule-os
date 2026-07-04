mod build;
mod cli;
mod output;
mod platform;
mod run;
mod toolchain;

use clap::Parser;
use cli::{Cli, Commands};
use platform::Platform;

fn main() {
    let cli = Cli::parse();
    match &cli.command {
        Commands::CheckToolchain { expected_rust } => {
            match toolchain::check_toolchain(expected_rust.as_deref()) {
                Ok(info) => {
                    println!("Rust: {}", info.rustc_version);
                    println!("rust-lld: {:?}", info.rust_lld_path);
                    println!("llvm-objcopy: {:?}", info.llvm_objcopy_path);
                }
                Err(e) => eprintln!("Toolchain check failed: {}", e),
            }
        }
        Commands::Build { arch } | Commands::Run { arch } => {
            let plat = match Platform::for_arch(arch) {
                Some(p) => p,
                None => {
                    eprintln!("Unsupported architecture: {}", arch);
                    std::process::exit(1);
                }
            };
            if let Err(e) = build::build(&plat) {
                eprintln!("Build failed: {}", e);
                std::process::exit(1);
            }
            if matches!(cli.command, Commands::Run { .. }) {
                if let Err(e) = run::run(&plat) {
                    eprintln!("Run failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
