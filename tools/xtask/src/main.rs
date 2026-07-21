mod build;
mod clean;
mod cli;
mod config;
mod output;
mod pack;
mod platform;
mod run;
mod toolchain;

use clap::Parser;
use cli::{Cli, CodeSubcommands, Commands};
use config::Config;
use platform::Platform;

fn main() {
    if std::env::var("CARGO_MANIFEST_DIR").is_ok() {
        eprintln!("\x1b[1;31m❌ Error: Running xtask via 'cargo run' or 'cargo xtask' is strictly disabled!\x1b[0m");
        eprintln!("\x1b[1;33m💡 Please compile and run xtask as a direct binary instead.\x1b[0m");
        eprintln!("\x1b[1;32m💡 Run the following script to compile and install the release binary:\x1b[0m");
        eprintln!("   \x1b[1;36m./install_xtask\x1b[0m");
        std::process::exit(1);
    }

    let cli = Cli::parse();

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Configuration load failed: {}", e);
            std::process::exit(1);
        }
    };

    match &cli.command {
        Commands::Clean => {
            if let Err(e) = clean::clean() {
                eprintln!("Clean failed: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Code { sub } => match sub {
            CodeSubcommands::CheckEnv { expected_rust } => {
                match toolchain::check_toolchain(expected_rust.as_deref()) {
                    Ok(info) => {
                        println!("Rust: {}", info.rustc_version);
                        println!("rust-lld: {:?}", info.rust_lld_path);
                        println!("llvm-objcopy: {:?}", info.llvm_objcopy_path);
                    }
                    Err(e) => {
                        eprintln!("Environment check failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            CodeSubcommands::Build { arch, platform } => {
                let plat = match Platform::from_config(arch, platform, &config) {
                    Some(p) => p,
                    None => {
                        eprintln!("Unsupported architecture/platform: {}/{}", arch, platform);
                        std::process::exit(1);
                    }
                };
                if let Err(e) = build::build(&config, &plat) {
                    eprintln!("Build failed: {}", e);
                    std::process::exit(1);
                }
            }
            CodeSubcommands::Run { arch, platform, gdb } => {
                let plat = match Platform::from_config(arch, platform, &config) {
                    Some(p) => p,
                    None => {
                        eprintln!("Unsupported architecture/platform: {}/{}", arch, platform);
                        std::process::exit(1);
                    }
                };
                if let Err(e) = build::build(&config, &plat) {
                    eprintln!("Build failed: {}", e);
                    std::process::exit(1);
                }
                if let Err(e) = run::run(&config, &plat, *gdb) {
                    eprintln!("Run failed: {}", e);
                    std::process::exit(1);
                }
            }
        },
    }
}
