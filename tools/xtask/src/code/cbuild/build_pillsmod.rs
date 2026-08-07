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

        let display_str = v.to_display_string();
        cmd.env("CAPSULEOS_BUILD_NUM", &v.patch);
        cmd.env("CAPSULEOS_VERSION", &display_str);

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

        let clean_lib_name = bin_name.replace("-", "_");
        let lib_path = format!("build/target/{}/release/lib{}.a", plat.rust_target, clean_lib_name);
        
        let dist_dir = format!("{}/extensions", crate::config::BUILD_DIST);
        std::fs::create_dir_all(&dist_dir).map_err(|e| e.to_string())?;

        let elf_path = format!("{}/{}.elf", dist_dir, bin_name);
        let raw_path = format!("{}/{}.raw", dist_dir, bin_name);

        // Link .a into ELF
        let lld = crate::code::toolchain::find_rust_lld();
        let result = run_silent(
            Command::new(&lld).args([
                "-flavor",
                "gnu",
                "-m",
                &plat.ld_emulation,
                "--gc-sections",
                "--whole-archive",
                "-T",
                &plat.linker_script,
                &lib_path,
                "--no-whole-archive",
                "-o",
                &elf_path,
            ]),
            || {},
        );
        if !result.success {
            println!();
            return Err(format!("failed to link pillsmod {}", bin_name));
        }

        // Extract raw binary payload
        let objcopy = find_objcopy();
        let result = run_silent(
            Command::new(&objcopy).args(["-O", "binary", &elf_path, &raw_path]),
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

        // Assemble .pill package
        let mut pill_bytes = Vec::new();
        let magic = b"PILL";
        let v_major = 1u16;
        let v_minor = 0u16;
        let metadata_bytes = metadata_content.as_bytes();
        
        let metadata_offset = 32u64;
        let metadata_size = metadata_bytes.len() as u64;
        let payload_offset = metadata_offset + metadata_size;
        let payload_size = payload.len() as u64;

        // Header (32 bytes)
        pill_bytes.extend_from_slice(magic);
        pill_bytes.extend_from_slice(&v_major.to_le_bytes());
        pill_bytes.extend_from_slice(&v_minor.to_le_bytes());
        pill_bytes.extend_from_slice(&metadata_offset.to_le_bytes());
        pill_bytes.extend_from_slice(&metadata_size.to_le_bytes());
        pill_bytes.extend_from_slice(&payload_offset.to_le_bytes());
        pill_bytes.extend_from_slice(&payload_size.to_le_bytes());

        // Metadata
        pill_bytes.extend_from_slice(metadata_bytes);

        // Payload
        pill_bytes.extend_from_slice(&payload);

        let output_resolved = resolve_path_placeholders(&item.output)
            .replace("*", &bin_name);

        if let Some(parent) = Path::new(&output_resolved).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        std::fs::write(&output_resolved, &pill_bytes).map_err(|e| e.to_string())?;
        println!("\r  Packing ({}/{}) {}.pill... DONE\x1B[K", pack_step, total_steps, bin_name);
    }
    Ok(())
}
