mod build;
mod clean;
mod cli;
mod config;
mod output;
mod pack;
mod platform;
mod repo;
mod run;
mod toolchain;

use clap::Parser;
use cli::{Cli, CodeSubcommands, Commands};
use config::Config;
use platform::Platform;

fn main() {
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
            CodeSubcommands::Build { arch } => {
                let plat = match Platform::from_config(arch, &config) {
                    Some(p) => p,
                    None => {
                        eprintln!("Unsupported architecture: {}", arch);
                        std::process::exit(1);
                    }
                };
                if let Err(e) = build::build(&config, &plat) {
                    eprintln!("Build failed: {}", e);
                    std::process::exit(1);
                }
            }
            CodeSubcommands::Run { arch, gdb } => {
                let plat = match Platform::from_config(arch, &config) {
                    Some(p) => p,
                    None => {
                        eprintln!("Unsupported architecture: {}", arch);
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
        Commands::Repo { sub } => {
            if let Err(e) = repo::handle_repo(sub) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
    }
}
