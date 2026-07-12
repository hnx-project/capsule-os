use crate::repo::{is_admin_mode, run_cmd, run_cmd_status, GitCodeSession, XtaskConfig};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::ZipWriter;

#[derive(Deserialize, Debug)]
struct CreateReleaseResponse {
    id: Option<u64>,
    html_url: Option<String>,
}

fn zip_release_assets(
    zip_path: &Path,
    aarch64_ohc: &Path,
    aarch64_bin: &Path,
    riscv64_ohc: &Path,
    riscv64_bin: &Path,
) -> Result<Vec<u8>, String> {
    let file =
        fs::File::create(zip_path).map_err(|e| format!("Failed to create ZIP output: {}", e))?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    zip.start_file("aarch64/hnxcore", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data =
        fs::read(aarch64_ohc).map_err(|e| format!("Failed to read AArch64 OHC capsule: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.start_file("aarch64/capsule-bootloader.bin", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data =
        fs::read(aarch64_bin).map_err(|e| format!("Failed to read AArch64 Bootloader: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.start_file("riscv64/hnxcore", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data = fs::read(riscv64_ohc)
        .map_err(|e| format!("Failed to read RISC-V 64 OHC capsule: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.start_file("riscv64/capsule-bootloader.bin", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    let data =
        fs::read(riscv64_bin).map_err(|e| format!("Failed to read RISC-V 64 Bootloader: {}", e))?;
    zip.write_all(&data).unwrap();

    zip.finish()
        .map_err(|e| format!("Failed to finalize ZIP archive: {}", e))?;

    let zip_bytes =
        fs::read(zip_path).map_err(|e| format!("Failed to read final ZIP binary bytes: {}", e))?;
    Ok(zip_bytes)
}

pub fn handle_release(
    version: &str,
    config: &XtaskConfig,
    session: &GitCodeSession,
) -> Result<(), String> {
    if !is_admin_mode(config) {
        return Err(
            "❌ Unauthorized: Only administrators can create and publish Production Releases."
                .to_string(),
        );
    }

    println!(
        "🎁 Starting production release publication for {}...",
        version
    );

    println!("⚙️ [1/3] Building AArch64 target (release mode)...");
    run_cmd_status(&["cargo", "xtask", "build", "--arch", "aarch64"], None)
        .map_err(|_| "❌ Failed to compile AArch64 targets!".to_string())?;

    let aarch64_ohc = Path::new("build/dist/kernel/hnxcore");
    let aarch64_bin = Path::new("build/target/aarch64-unknown-none/release/capsule-bootloader.bin");

    let temp_aarch64_ohc = Path::new("build/hnxcore_aarch64");
    let temp_aarch64_bin = Path::new("build/capsule-bootloader_aarch64.bin");
    let _ = fs::copy(aarch64_ohc, temp_aarch64_ohc);
    let _ = fs::copy(aarch64_bin, temp_aarch64_bin);

    println!("⚙️ [2/3] Building RISC-V 64 target (release mode)...");
    run_cmd_status(&["cargo", "xtask", "build", "--arch", "riscv64"], None)
        .map_err(|_| "❌ Failed to compile RISC-V 64 targets!".to_string())?;

    let riscv64_ohc = Path::new("build/dist/kernel/hnxcore");
    let riscv64_bin =
        Path::new("build/target/riscv64imac-unknown-none-elf/release/capsule-bootloader.bin");

    println!("📦 [3/3] Archiving and compressing all targets to standard ZIP...");

    let date_str = run_cmd(&["date", "+%Y%m%d"], None).unwrap_or_else(|_| "20260706".to_string());

    let zip_name = format!(
        "capsuleos-{}-{}-hnx-{}.zip",
        config.project.codename, version, date_str
    );
    let zip_path = PathBuf::from(format!("build/dist/distribution/{}", zip_name));

    if let Some(parent) = zip_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let zip_bytes = zip_release_assets(
        &zip_path,
        temp_aarch64_ohc,
        temp_aarch64_bin,
        riscv64_ohc,
        riscv64_bin,
    )?;

    let _ = fs::remove_file(temp_aarch64_ohc);
    let _ = fs::remove_file(temp_aarch64_bin);

    let zip_size_mb = zip_bytes.len() as f64 / 1024.0 / 1024.0;
    println!(
        "💾 Archive ZIP created: \x1B[1;36mbuild/dist/distribution/{}\x1B[0m ({:.2} MB)",
        zip_name, zip_size_mb
    );

    println!("📨 Creating Release page on GitCode for tag {}...", version);
    let create_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/releases?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, session.token
    );

    let payload = serde_json::json!({
        "tag_name": version,
        "name": format!("CapsuleOS {} - Pangu Release", version),
        "body": format!("### 🚀 CapsuleOS {} Production Release\n\n\
                        This release has been automatically compiled, packed, and published via `cargo xtask repo release {}`.\n\n\
                        #### 📦 Attached Assets (Multi-Arch):\n\
                        - `aarch64/hnxcore` (Kernel OHLINK Capsule)\n\
                        - `aarch64/capsule-bootloader.bin` (Firmware Shim Bootloader)\n\
                        - `riscv64/hnxcore` (Kernel OHLINK Capsule)\n\
                        - `riscv64/capsule-bootloader.bin` (Firmware Shim Bootloader)\n\n\
                        All assets are unified and zipped inside **`{}`** below.", version, version, zip_name),
        "prerelease": false,
        "target_commitish": "main"
    });

    let resp = match ureq::post(&create_url).send_json(payload) {
        Ok(r) => r,
        Err(e) => return Err(format!("❌ Failed to create GitCode Release: {}", e)),
    };

    let release_info: CreateReleaseResponse = resp
        .into_json()
        .map_err(|e| format!("Failed to parse release response: {}", e))?;
    let release_id = match release_info.id {
        Some(id) => id,
        None => return Err("GitCode response did not contain a valid release id.".to_string()),
    };

    println!("📎 Uploading release archive ZIP to GitCode (this may take a few seconds)...");
    let upload_url = format!(
        "https://gitcode.com/api/v5/repos/{}/{}/releases/{}/attach_files?access_token={}",
        config.gitcode.upstream_owner, config.gitcode.upstream_repo, release_id, session.token
    );

    let boundary = "------------------------capsuleosreleaseboundary123456789";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            zip_name
        )
        .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/zip\r\n\r\n");
    body.extend_from_slice(&zip_bytes);
    body.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());

    match ureq::post(&upload_url)
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
    {
        Ok(resp) => {
            if resp.status() == 201 || resp.status() == 200 {
                println!("\n🚀 \x1B[1;32mProduction Release & Attachments successfully published!\x1B[0m");
                if let Some(url) = release_info.html_url {
                    println!("👉 View Release page: \x1B[1;36m{}\x1B[0m\n", url);
                }
            } else {
                return Err(format!(
                    "❌ Upload asset failed with status code: {}",
                    resp.status()
                ));
            }
        }
        Err(e) => return Err(format!("❌ Failed to upload ZIP asset: {}", e)),
    }

    Ok(())
}
