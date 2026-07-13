mod build;
mod clean;
mod cli;
mod output;
mod pack;
mod platform;
mod repo;
mod run;
mod toolchain;

use clap::Parser;
use cli::{Cli, CodeSubcommands, Commands};
use platform::Platform;

fn main() {
    let cli = Cli::parse();
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
            }
            CodeSubcommands::Run { arch, gdb } => {
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
                if let Err(e) = run::run(&plat, *gdb) {
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
