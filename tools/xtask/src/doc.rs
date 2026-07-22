use std::fs;
use std::path::Path;
use std::process::Command;
use crate::config::Config;

pub fn generate_doc(_config: &Config) -> Result<(), String> {
    println!("\x1b[1;36m🏗️  Generating CapsuleOS Unified API Documentation...\x1b[0m");

    // 1. Clean and initialize build/dist/docs directory
    let docs_dist_dir = "build/dist/docs";
    if Path::new(docs_dist_dir).exists() {
        fs::remove_dir_all(docs_dist_dir).map_err(|e| format!("Failed to clear existing docs dir: {}", e))?;
    }
    fs::create_dir_all(docs_dist_dir).map_err(|e| format!("Failed to create docs distribution dir: {}", e))?;

    // 2. Generate L1 Kernel doc
    println!("📖 [\x1b[1;32m1/4\x1b[0m] Generating L1 Microkernel HNX Core documentation...");
    let mut cmd_kernel = Command::new("cargo");
    cmd_kernel.args([
        "doc",
        "--target",
        "aarch64-unknown-none",
        "--no-deps",
    ])
    .current_dir("kernel");
    
    let status_kernel = cmd_kernel.status()
        .map_err(|e| format!("Failed to run cargo doc for kernel: {}", e))?;
    if !status_kernel.success() {
        return Err("Failed to generate kernel documentation".to_string());
    }

    // 3. Generate L2/L3 Userspace doc
    println!("📖 [\x1b[1;32m2/4\x1b[0m] Generating L2/L3 Userspace Runtime & Programs documentation...");
    let mut cmd_userspace = Command::new("cargo");
    cmd_userspace.args([
        "+nightly",
        "doc",
        "--workspace",
        "--target",
        "libraries/targets/aarch64-unknown-capsule.json",
        "--no-deps",
        "--no-default-features",
        "--features", "capsule",
        "-Z", "build-std=core,alloc,panic_abort",
        "-Z", "json-target-spec",
        "--exclude", "xtask",
        "--exclude", "ohlink-format",
        "--exclude", "ohlink-linker",
        "--exclude", "ohlink-read",
    ]);
    
    let status_userspace = cmd_userspace.status()
        .map_err(|e| format!("Failed to run cargo doc for userspace: {}", e))?;
    if !status_userspace.success() {
        return Err("Failed to generate userspace documentation".to_string());
    }

    // 4. Generate Host Tools doc
    println!("📖 [\x1b[1;32m3/4\x1b[0m] Generating Host Orchestration & Dev Tools documentation...");
    let mut cmd_tools = Command::new("cargo");
    cmd_tools.args([
        "doc",
        "--manifest-path",
        "tools/xtask/Cargo.toml",
        "--no-deps",
    ]);
    
    let status_tools = cmd_tools.status()
        .map_err(|e| format!("Failed to run cargo doc for host tools: {}", e))?;
    if !status_tools.success() {
        return Err("Failed to generate host tools documentation".to_string());
    }

    // 5. Generate Ohlink Toolchain doc
    println!("📖 [\x1b[1;32m4/4\x1b[0m] Generating OHLINK Toolchain compilation engine documentation...");
    let mut cmd_toolchain = Command::new("cargo");
    cmd_toolchain.args([
        "doc",
        "--manifest-path",
        "tools/ohlink-toolchain/Cargo.toml",
        "--no-deps",
    ]);
    
    let status_toolchain = cmd_toolchain.status()
        .map_err(|e| format!("Failed to run cargo doc for ohlink-toolchain: {}", e))?;
    if !status_toolchain.success() {
        return Err("Failed to generate ohlink-toolchain documentation".to_string());
    }

    // 6. Gather and collect docs with multi-path fallback
    println!("\x1b[1;36m📥 Collecting compiled HTML packages...\x1b[0m");
    
    // Kernel collection
    find_and_copy_doc(
        &[
            "kernel/build/target/aarch64-unknown-none/doc",
            "kernel/target/aarch64-unknown-none/doc",
            "build/target/aarch64-unknown-none/doc",
        ],
        &format!("{}/kernel", docs_dist_dir),
    )?;

    // Userspace collection
    find_and_copy_doc(
        &[
            "build/target/aarch64-unknown-capsule/doc",
            "target/aarch64-unknown-capsule/doc",
        ],
        &format!("{}/userspace", docs_dist_dir),
    )?;

    // Host Tools collection
    find_and_copy_doc(
        &[
            "build/target/doc",
            "target/doc",
        ],
        &format!("{}/host_tools", docs_dist_dir),
    )?;

    // Ohlink Toolchain collection
    find_and_copy_doc(
        &[
            "tools/ohlink-toolchain/target/doc",
            "build/target/doc",
            "target/doc",
        ],
        &format!("{}/ohlink_toolchain", docs_dist_dir),
    )?;

    // 7. Generate beautiful dark-themed unified index.html landing page
    println!("\x1b[1;36m✨ Generating unified documentation portal...\x1b[0m");
    let index_html_content = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>CapsuleOS (Pangu) Developer Documentation Hub</title>
    <style>
        :root {
            --bg-color: #0d1117;
            --card-bg: #161b22;
            --text-color: #c9d1d9;
            --accent-color: #58a6ff;
            --accent-purple: #bc8cff;
            --accent-green: #3fb950;
            --accent-orange: #f0883e;
            --border-color: #30363d;
        }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
            background-color: var(--bg-color);
            color: var(--text-color);
            margin: 0;
            padding: 40px 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
        }
        header {
            text-align: center;
            margin-bottom: 50px;
            max-width: 800px;
        }
        h1 {
            font-size: 2.5rem;
            color: #ffffff;
            margin-bottom: 10px;
            font-weight: 700;
            letter-spacing: -0.5px;
        }
        .subtitle {
            font-size: 1.1rem;
            color: #8b949e;
            line-height: 1.5;
        }
        .grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 24px;
            max-width: 1200px;
            width: 100%;
        }
        .card {
            background-color: var(--card-bg);
            border: 1px solid var(--border-color);
            border-radius: 12px;
            padding: 24px;
            transition: all 0.3s ease;
            text-decoration: none;
            color: inherit;
            display: flex;
            flex-direction: column;
            height: 100%;
            box-sizing: border-box;
        }
        .card:hover {
            transform: translateY(-4px);
            border-color: var(--accent-color);
            box-shadow: 0 8px 24px rgba(56, 139, 253, 0.15);
        }
        .card-title {
            font-size: 1.25rem;
            font-weight: 600;
            margin: 0 0 12px 0;
            color: #ffffff;
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .card-desc {
            font-size: 0.9rem;
            color: #8b949e;
            line-height: 1.5;
            flex-grow: 1;
            margin-bottom: 20px;
        }
        .card-badge {
            font-size: 0.75rem;
            font-weight: 600;
            padding: 4px 8px;
            border-radius: 6px;
            align-self: flex-start;
            text-transform: uppercase;
            letter-spacing: 0.5px;
        }
        .badge-l1 { background-color: rgba(188, 140, 255, 0.15); color: var(--accent-purple); border: 1px solid rgba(188, 140, 255, 0.3); }
        .badge-l2 { background-color: rgba(56, 139, 253, 0.15); color: var(--accent-color); border: 1px solid rgba(56, 139, 253, 0.3); }
        .badge-host { background-color: rgba(63, 185, 80, 0.15); color: var(--accent-green); border: 1px solid rgba(63, 185, 80, 0.3); }
        .badge-tool { background-color: rgba(240, 136, 62, 0.15); color: var(--accent-orange); border: 1px solid rgba(240, 136, 62, 0.3); }
        footer {
            margin-top: 80px;
            color: #484f58;
            font-size: 0.85rem;
            text-align: center;
        }
    </style>
</head>
<body>
    <header>
        <h1>🌌 CapsuleOS (Pangu) Developer Hub</h1>
        <div class="subtitle">Unified API Documentation and Reference manual for CapsuleOS, HNX Microkernel, standard runtime libraries, userspace sandboxes, and customized toolchains.</div>
    </header>
    <div class="grid">
        <a href="./kernel/kernel/index.html" class="card">
            <div class="card-title">🛡️ HNX Microkernel</div>
            <div class="card-desc">Low-level operating system microkernel core running at EL1. Contains process schedules, virtual memory (VMAR/VMO), zero-allocation IPC, and capabilities handle system.</div>
            <div class="card-badge badge-l1">L1 Privileged Core</div>
        </a>
        <a href="./userspace/libc/index.html" class="card">
            <div class="card-title">🧬 Userspace Standard Runtime</div>
            <div class="card-desc">C-ABI POSIX shim translation library (libc), safe capability wrapping runtime (libcapsule), and custom safe Rust standard library (libstd).</div>
            <div class="card-badge badge-l2">L2 Runtime & APIs</div>
        </a>
        <a href="./host_tools/xtask/index.html" class="card">
            <div class="card-title">⚙️ Xtask Build Orchestrator</div>
            <div class="card-desc">Host developer task CLI tool for compiling, cleaning, local QEMU simulation, firmware fetching, dynamic MBR formatting, and version audits.</div>
            <div class="card-badge badge-host">Host Development</div>
        </a>
        <a href="./ohlink_toolchain/ohlink_format/index.html" class="card">
            <div class="card-title">⛓️ OHLINK Toolchain</div>
            <div class="card-desc">Custom binary format specification (OHLINK), compiler codegen plugin for rustc, and low-level zero-ELF absolute binary linker.</div>
            <div class="card-badge badge-tool">Custom Toolchain</div>
        </a>
    </div>
    <footer>
        Developed by HNX-Project. Powered by Rust & cargo-doc.
    </footer>
</body>
</html>
"#;

    let index_path = format!("{}/index.html", docs_dist_dir);
    fs::write(&index_path, index_html_content)
        .map_err(|e| format!("Failed to write index.html landing page: {}", e))?;

    println!("🎉 \x1b[1;32mUnified rust-doc website compiled successfully at: {}/\x1b[0m", docs_dist_dir);
    Ok(())
}

fn find_and_copy_doc(paths: &[&str], dest: &str) -> Result<(), String> {
    let mut found = false;
    for path in paths {
        let p = Path::new(path);
        if p.exists() && p.is_dir() {
            println!("📥 Collecting documentation from \x1b[1;34m{}\x1b[0m -> \x1b[1;32m{}\x1b[0m", path, dest);
            if Path::new(dest).exists() {
                fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
            }
            copy_dir_all(p, dest).map_err(|e| format!("Failed to copy doc from {} to {}: {}", path, dest, e))?;
            found = true;
            break;
        }
    }
    if !found {
        println!("⚠️  \x1b[1;33mWarning: Could not find generated documentation in any of these paths: {:?}\x1b[0m", paths);
    }
    Ok(())
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}