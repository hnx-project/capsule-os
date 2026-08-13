use std::process::Command;
use std::path::Path;
use crate::config::{Resolved, ParsedVersion, resolve_path_placeholders, parse_cargo_toml};
use crate::platform::Platform;
use crate::output::run_silent;
use crate::code::toolchain::find_objcopy;

pub fn build_pillsmod(
    resolved: &Resolved,
    plat: &Platform,
    v: &ParsedVersion,
    current_step: &mut u32,
    total_steps: u32,
) -> Result<(), String> {
    for item in &resolved.build.pillsmod {
        let crate_dir = Path::new(&item.path).parent().ok_or_else(|| {
            "pillsmod path must point to a valid Cargo.toml".to_string()
        })?;

        let (crate_name, bin_name) = parse_cargo_toml(&item.path)?;

        // Step 1: Compiling
        let comp_step = *current_step;
        *current_step += 1;
        print!("  Compiling ({}/{}) pillsmod {}...", comp_step, total_steps, crate_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let mut cmd = Command::new("cargo");
        cmd.args([
            "build",
            "--target",
            &plat.rust_target,
            "--release",
        ])
        .current_dir(crate_dir);

        for (key, _) in std::env::vars() {
            if key.starts_with("CARGO") {
                cmd.env_remove(&key);
            }
        }

        let display_str = v.to_display_string();
        cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
        cmd.env("CAPSULEOS_VERSION", &display_str);
        cmd.env("RUSTFLAGS", "-C link-arg=--image-base=0 -C link-arg=-Ttext=0");

        let result = run_silent(&mut cmd, || {});
        if !result.success {
            println!();
            return Err(format!("failed to build pillsmod {}", crate_name));
        }
        println!("\r  Compiling ({}/{}) pillsmod {}... DONE\x1B[K", comp_step, total_steps, crate_name);

        // Step 2: Extracting raw payload and packing into .pill
        let pack_step = *current_step;
        *current_step += 1;
        print!("  Packing ({}/{}) {}.pill...", pack_step, total_steps, bin_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let dist_dir = format!("{}/extensions", crate::config::BUILD_DIST);
        std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

        let elf_path = format!("build/target/{}/release/{}", plat.rust_target, bin_name);
        let raw_path = format!("{}/{}.raw", dist_dir, bin_name);

        // Extract raw binary payload from cargo-compiled ELF directly (no manual lld)
        let objcopy = find_objcopy();
        let result = run_silent(
            Command::new(&objcopy).args(["-O", "binary", "-R", ".eh_frame", "-R", ".eh_frame_hdr", &elf_path, &raw_path]),
            || {},
        );
        if !result.success {
            println!();
            return Err(format!("failed to extract raw binary for pillsmod {}", bin_name));
        }

        let payload = std::fs::read(&raw_path).map_err(|e| e.to_string())?;

        // Read/generate metadata.toml
        let metadata_path = crate_dir.join("metadata.toml");
        let metadata_content = if metadata_path.exists() {
            std::fs::read_to_string(&metadata_path).map_err(|e| e.to_string())?
        } else {
            format!(
                "[driver]\nname = \"{}\"\nversion = \"{}\"\nclass = \"PillsMod\"\nentry_symbol = \"pillsmod_init\"\n",
                bin_name, v.to_display_string()
            )
        };

        // Create directory bundle under extensions
        let output_resolved = resolve_path_placeholders(&item.output)
            .replace("*", &bin_name);

        std::fs::create_dir_all(&output_resolved).map_err(|e| e.to_string())?;

        // 1. Write metadata.toml inside the bundle
        let meta_dest_path = Path::new(&output_resolved).join("metadata.toml");
        std::fs::write(&meta_dest_path, &metadata_content).map_err(|e| e.to_string())?;

        // 2. Write driver inside the bundle
        let raw_dest_path = Path::new(&output_resolved).join("driver");
        std::fs::write(&raw_dest_path, &payload).map_err(|e| e.to_string())?;

        let bundle_name = Path::new(&output_resolved).file_name().and_then(|n| n.to_str()).unwrap_or(&bin_name);
        println!("\r  Packing ({}/{}) {}... DONE\x1B[K", pack_step, total_steps, bundle_name);
    }
    Ok(())
}
