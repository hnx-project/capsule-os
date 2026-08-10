use std::process::Command;
use std::path::Path;
use crate::config::{Resolved, ParsedVersion, resolve_path_placeholders, parse_cargo_toml};
use crate::platform::Platform;
use crate::output::run_silent;

pub fn build_services(
    resolved: &Resolved,
    plat: &Platform,
    v: &ParsedVersion,
    current_step: &mut u32,
    total_steps: u32,
) -> Result<(), String> {
    for item in &resolved.build.services {
        let crate_dir = Path::new(&item.path).parent().ok_or_else(|| {
            "service path must point to a valid Cargo.toml".to_string()
        })?;

        let (crate_name, bin_name) = parse_cargo_toml(&item.path)?;

        // Step 1: Compiling
        let comp_step = *current_step;
        *current_step += 1;
        print!("  Compiling ({}/{}) service {}...", comp_step, total_steps, crate_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let display_str = v.to_display_string();
        let absolute_target = std::fs::canonicalize(&plat.userspace_target)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| plat.userspace_target.clone());

        let mut cmd = Command::new("cargo");
        cmd.args([
            "+nightly",
            "build",
            "--release",
            "-p",
            &crate_name,
            "--target",
            &absolute_target, // Use absolute path
            "--no-default-features",
            "--features",
            "capsule",
            "-Z",
            "build-std=core,alloc,panic_abort",
            "-Z",
            "json-target-spec",
        ])
        .current_dir(crate_dir);

        cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
        cmd.env("CAPSULEOS_VERSION", &display_str);

        let result = run_silent(&mut cmd, || {});
        if !result.success {
            println!();
            return Err(format!("failed to build service {}", crate_name));
        }
        println!("\r  Compiling ({}/{}) service {}... DONE\x1B[K", comp_step, total_steps, crate_name);

        // Step 2: Packing into OHLINK
        let pack_step = *current_step;
        *current_step += 1;
        print!("  Packing ({}/{}) {}...", pack_step, total_steps, bin_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let elf_path = format!(
            "build/target/aarch64-unknown-capsule/release/{}",
            crate_name.replace("hnx-", "")
        );

        let output_resolved = resolve_path_placeholders(&item.output);
        if let Some(parent) = Path::new(&output_resolved).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let result = run_silent(
            Command::new("cargo").args([
                "run",
                "--manifest-path",
                "tools/ohlink-toolchain/Cargo.toml",
                "-p",
                "ohlink-linker",
                "--",
                "--input",
                &elf_path,
                "--output",
                &output_resolved,
                "--entry",
                "65536", // 固化标准用户态入口 0x10000
            ]),
            || {},
        );
        if !result.success {
            println!();
            return Err(format!("failed to pack service {}", bin_name));
        }
        println!("\r  Packing ({}/{}) {}... DONE\x1B[K", pack_step, total_steps, bin_name);
    }
    Ok(())
}
