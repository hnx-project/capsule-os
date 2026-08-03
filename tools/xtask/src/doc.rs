use crate::config::RootConfig;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::thread;

pub fn generate_doc(open: bool, _config: &RootConfig) -> Result<(), String> {
    println!("\x1b[1;36m🏗️  Generating CapsuleOS Unified API Documentation...\x1b[0m");

    // 1. Clean and initialize build/dist/docs directory
    let docs_dist_dir = "build/dist/docs";
    if Path::new(docs_dist_dir).exists() {
        fs::remove_dir_all(docs_dist_dir)
            .map_err(|e| format!("Failed to clear existing docs dir: {}", e))?;
    }
    fs::create_dir_all(docs_dist_dir)
        .map_err(|e| format!("Failed to create docs distribution dir: {}", e))?;

    // 2. Generate L1 Kernel doc
    println!("📖 [\x1b[1;32m1/4\x1b[0m] Generating L1 Microkernel HNX Core documentation...");
    let mut cmd_kernel = Command::new("cargo");
    cmd_kernel
        .args(["doc", "--target", "aarch64-unknown-none", "--no-deps"])
        .current_dir("kernel");

    let status_kernel = cmd_kernel
        .status()
        .map_err(|e| format!("Failed to run cargo doc for kernel: {}", e))?;
    if !status_kernel.success() {
        return Err("Failed to generate kernel documentation".to_string());
    }

    // 3. Generate L2/L3 Userspace doc
    println!(
        "📖 [\x1b[1;32m2/4\x1b[0m] Generating L2/L3 Userspace Runtime & Programs documentation..."
    );
    let mut cmd_userspace = Command::new("cargo");
    cmd_userspace.args([
        "+nightly",
        "doc",
        "--workspace",
        "--target",
        "libraries/targets/aarch64-unknown-capsule.json",
        "--no-deps",
        "--no-default-features",
        "--features",
        "capsule",
        "-Z",
        "build-std=core,alloc,panic_abort",
        "-Z",
        "json-target-spec",
        "--exclude",
        "xtask",
        "--exclude",
        "ohlink-format",
        "--exclude",
        "ohlink-linker",
        "--exclude",
        "ohlink-read",
    ]);

    let status_userspace = cmd_userspace
        .status()
        .map_err(|e| format!("Failed to run cargo doc for userspace: {}", e))?;
    if !status_userspace.success() {
        return Err("Failed to generate userspace documentation".to_string());
    }

    // 4. Generate Host Tools doc
    println!(
        "📖 [\x1b[1;32m3/4\x1b[0m] Generating Host Orchestration & Dev Tools documentation..."
    );
    let mut cmd_tools = Command::new("cargo");
    cmd_tools.args([
        "doc",
        "--manifest-path",
        "tools/xtask/Cargo.toml",
        "--no-deps",
    ]);

    let status_tools = cmd_tools
        .status()
        .map_err(|e| format!("Failed to run cargo doc for host tools: {}", e))?;
    if !status_tools.success() {
        return Err("Failed to generate host tools documentation".to_string());
    }

    // 5. Generate Ohlink Toolchain doc
    println!(
        "📖 [\x1b[1;32m4/4\x1b[0m] Generating OHLINK Toolchain compilation engine documentation..."
    );
    let mut cmd_toolchain = Command::new("cargo");
    cmd_toolchain.args([
        "doc",
        "--manifest-path",
        "tools/toolchain/Cargo.toml",
        "--no-deps",
    ]);

    let status_toolchain = cmd_toolchain
        .status()
        .map_err(|e| format!("Failed to run cargo doc for toolchain: {}", e))?;
    if !status_toolchain.success() {
        return Err("Failed to generate toolchain documentation".to_string());
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
        &["build/target/doc", "target/doc"],
        &format!("{}/host_tools", docs_dist_dir),
    )?;

    // Ohlink Toolchain collection
    find_and_copy_doc(
        &[
            "tools/toolchain/target/doc",
            "build/target/doc",
            "target/doc",
        ],
        &format!("{}/ohlink_toolchain", docs_dist_dir),
    )?;

    // 7. Generate beautiful print-style unified index.html landing page
    println!("\x1b[1;36m✨ Generating unified documentation portal...\x1b[0m");
    let index_html_content = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>CapsuleOS (Pangu) Developer Documentation Portal</title>
    <style>
        :root {
            --bg-color: #faf9f6;
            --text-main: #111827;
            --text-sub: #4b5563;
            --text-muted: #9ca3af;
            --border-color: #e5e7eb;
            --accent-color: #df3625; /* Raspberry Pi / Rust Red */
            --accent-bg-hover: #f3f4f6;
        }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            background-color: var(--bg-color);
            color: var(--text-main);
            margin: 0;
            padding: 80px 20px;
            display: flex;
            flex-direction: column;
            align-items: center;
        }
        .container {
            max-width: 800px;
            width: 100%;
        }
        header {
            border-bottom: 2px solid var(--text-main);
            padding-bottom: 24px;
            margin-bottom: 40px;
        }
        h1 {
            font-family: "Source Serif 4", Georgia, serif;
            font-size: 2.5rem;
            font-weight: 500;
            margin: 0 0 12px 0;
            color: var(--text-main);
            letter-spacing: -0.5px;
        }
        .subtitle {
            font-size: 1.1rem;
            color: var(--text-sub);
            line-height: 1.6;
            font-family: "Source Serif 4", Georgia, serif;
            font-style: italic;
        }
        .section-title {
            font-family: monospace;
            font-size: 0.85rem;
            text-transform: uppercase;
            letter-spacing: 1px;
            color: var(--text-muted);
            margin: 40px 0 16px 0;
            border-bottom: 1px solid var(--border-color);
            padding-bottom: 8px;
        }
        .list {
            display: flex;
            flex-direction: column;
            gap: 16px;
        }
        .item {
            display: block;
            text-decoration: none;
            color: inherit;
            border: 1px solid var(--border-color);
            border-radius: 4px;
            padding: 20px;
            background-color: #ffffff;
            transition: all 0.2s ease;
        }
        .item:hover {
            border-color: var(--accent-color);
            background-color: #fffdfc;
        }
        .item-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 8px;
        }
        .item-title {
            font-size: 1.2rem;
            font-weight: 600;
            color: var(--text-main);
            font-family: "Source Serif 4", Georgia, serif;
        }
        .item:hover .item-title {
            color: var(--accent-color);
        }
        .item-badge {
            font-family: monospace;
            font-size: 0.75rem;
            color: var(--accent-color);
            border: 1px solid var(--accent-color);
            padding: 2px 8px;
            border-radius: 3px;
            background-color: rgba(223, 54, 37, 0.04);
        }
        .item:hover .item-badge {
            background-color: var(--accent-color);
            color: #ffffff;
        }
        .item-desc {
            font-size: 0.925rem;
            color: var(--text-sub);
            line-height: 1.5;
        }
        footer {
            margin-top: 80px;
            border-top: 1px solid var(--border-color);
            padding-top: 24px;
            color: var(--text-muted);
            font-family: monospace;
            font-size: 0.8rem;
            text-align: center;
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>capsuleOS / pangu</h1>
            <div class="subtitle">A from-scratch Unix-like microkernel operating system built in Rust for the AArch64 architecture.</div>
        </header>
        
        <div class="section-title">System Architecture Reference</div>
        <div class="list">
            <a href="./kernel/kernel/index.html" class="item">
                <div class="item-header">
                    <div class="item-title">L1 Privileged Microkernel (HNX Core)</div>
                    <div class="item-badge">hnxcore</div>
                </div>
                <div class="item-desc">Low-level microkernel core running at EL1. Manages thread scheduling, capabilities handle mappings, zero-allocation IPC channels, and physical memory allocation.</div>
            </a>
            <a href="./userspace/libc/index.html" class="item">
                <div class="item-header">
                    <div class="item-title">L2 Standard Runtime & L3 Sandboxed Services</div>
                    <div class="item-badge">userspace</div>
                </div>
                <div class="item-desc">Standard C-ABI compatibility interface (libc), Safe microkernel capability wrappers (libcapsule), customized safe Rust standard library (libstd), and standard sandboxed user services.</div>
            </a>
        </div>

        <div class="section-title">Development & Toolchain Reference</div>
        <div class="list">
            <a href="./host_tools/xtask/index.html" class="item">
                <div class="item-header">
                    <div class="item-title">Host Build & Test Orchestration</div>
                    <div class="item-badge">xtask</div>
                </div>
                <div class="item-desc">Unified host-side developer task runner. Handles platform configuration compilation, emulated runtime environment deployments, and disk formatting.</div>
            </a>
            <a href="./ohlink_toolchain/ohlink_format/index.html" class="item">
                <div class="item-header">
                    <div class="item-title">OHLINK Absolute Binary Toolchain</div>
                    <div class="item-badge">ohlink-cc</div>
                </div>
                <div class="item-desc">Custom decoupled absolute binary layout specification (OHLINK), compiler codegen plugin for rustc, and low-level physical linker.</div>
            </a>
        </div>

        <footer>
            pangu 1.0.0-beta4 / hnx-project / built with rust & cargo-doc
        </footer>
    </div>
</body>
</html>
"#;

    let index_path = format!("{}/index.html", docs_dist_dir);
    fs::write(&index_path, index_html_content)
        .map_err(|e| format!("Failed to write index.html landing page: {}", e))?;

    println!(
        "🎉 \x1b[1;32mUnified rust-doc website compiled successfully at: {}/\x1b[0m",
        docs_dist_dir
    );

    if open {
        serve_and_open_docs(docs_dist_dir)?;
    }

    Ok(())
}

fn serve_and_open_docs(doc_dir_str: &str) -> Result<(), String> {
    // 1. Bind to a random OS-assigned available local TCP port
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind local HTTP server port: {}", e))?;
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);

    println!(
        "\x1b[1;34m🚀 Starting zero-dependency HTTP server on {}...\x1b[0m",
        url
    );
    println!(
        "\x1b[1;33m💡 Server is hosting local files. Press Ctrl+C in your terminal to exit.\x1b[0m"
    );

    let doc_dir = doc_dir_str.to_string();
    // 2. Spawn multi-threaded file-serving HTTP handler
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let doc_dir_clone = doc_dir.clone();
                thread::spawn(move || {
                    let mut buffer = [0; 4096];
                    if let Ok(size) = stream.read(&mut buffer) {
                        let req = String::from_utf8_lossy(&buffer[..size]);
                        let mut path_part = "/";
                        if let Some(first_line) = req.lines().next() {
                            let parts: Vec<&str> = first_line.split_whitespace().collect();
                            if parts.len() >= 2 {
                                path_part = parts[1];
                            }
                        }

                        // Remove query parameters or fragments if any
                        let path_part_clean = path_part
                            .split('?')
                            .next()
                            .unwrap_or(path_part)
                            .split('#')
                            .next()
                            .unwrap_or(path_part);

                        // Percent decode URLs (e.g. "%20" to " ")
                        let decoded_path = url_decode(path_part_clean);

                        // Check if the requested path maps to a directory on disk
                        let disk_path = format!("{}{}", doc_dir_clone, decoded_path);
                        let disk_path_obj = Path::new(&disk_path);
                        if disk_path_obj.exists() && disk_path_obj.is_dir() {
                            // Directory must have trailing slash so relative CSS/JS paths resolve correctly in the browser
                            if !decoded_path.ends_with('/') {
                                let redirect_url = format!("{}/", path_part_clean);
                                let response = format!(
                                    "HTTP/1.1 301 Moved Permanently\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                    redirect_url
                                );
                                let _ = stream.write_all(response.as_bytes());
                                return;
                            }
                        }

                        let file_path_str = if decoded_path == "/" || decoded_path.ends_with('/') {
                            format!("{}{}index.html", doc_dir_clone, decoded_path)
                        } else {
                            format!("{}{}", doc_dir_clone, decoded_path)
                        };

                        let path = Path::new(&file_path_str);
                        if path.exists() && path.is_file() {
                            if let Ok(content) = fs::read(path) {
                                let mime = if file_path_str.ends_with(".html") {
                                    "text/html"
                                } else if file_path_str.ends_with(".css") {
                                    "text/css"
                                } else if file_path_str.ends_with(".js") {
                                    "application/javascript"
                                } else if file_path_str.ends_with(".png") {
                                    "image/png"
                                } else if file_path_str.ends_with(".svg") {
                                    "image/svg+xml"
                                } else if file_path_str.ends_with(".woff") {
                                    "font/woff"
                                } else if file_path_str.ends_with(".woff2") {
                                    "font/woff2"
                                } else {
                                    "application/octet-stream"
                                };

                                let response = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    mime, content.len()
                                );
                                let _ = stream.write_all(response.as_bytes());
                                let _ = stream.write_all(&content);
                            }
                        } else {
                            let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nNot Found";
                            let _ = stream.write_all(response.as_bytes());
                        }
                    }
                });
            }
        }
    });

    // 3. Open platform default browser on macOS, Linux, or Windows
    println!("\x1b[1;36m🌐 Launching default web browser...\x1b[0m");
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(&url).status();

    #[cfg(target_os = "linux")]
    let _ = Command::new("xdg-open").arg(&url).status();

    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd").args(["/C", "start", &url]).status();

    // 4. Block on the main thread so the server doesn't shut down immediately
    loop {
        thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn url_decode(s: &str) -> String {
    let mut res = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                res.push(val);
                i += 3;
                continue;
            }
        }
        res.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&res).into_owned()
}

fn find_and_copy_doc(paths: &[&str], dest: &str) -> Result<(), String> {
    let mut found = false;
    for path in paths {
        let p = Path::new(path);
        if p.exists() && p.is_dir() {
            println!(
                "📥 Collecting documentation from \x1b[1;34m{}\x1b[0m -> \x1b[1;32m{}\x1b[0m",
                path, dest
            );
            if Path::new(dest).exists() {
                fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
            }
            copy_dir_all(p, dest)
                .map_err(|e| format!("Failed to copy doc from {} to {}: {}", path, dest, e))?;
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
