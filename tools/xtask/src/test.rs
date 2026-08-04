use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Resolved;
use crate::platform::Platform;

const DEFAULT_TIMEOUT_SECS: u64 = 90;

#[derive(Debug)]
pub struct TestReport {
    pub passed: u32,
    pub failed: u32,
    pub total: u32,
    pub log_tail: Vec<String>,
}

pub fn test(resolved: &Resolved, plat: &Platform, timeout_secs: u64) -> Result<(), String> {
    // Make sure everything is built
    if let Err(e) = crate::build::build(resolved, plat, false, true) {
        return Err(format!("prebuild failed: {}", e));
    }

    let boot_bin_owned = format!("{}/aarch64-unknown-none/release/capsule-bootloader.bin", crate::config::BUILD_TARGET);
    let boot_bin = std::path::Path::new(&boot_bin_owned);

    let kernel_bin_owned = "build/dist/kernel/hnxcore".to_string();
    let kernel_bin = std::path::Path::new(&kernel_bin_owned);

    let rootfs_img_owned = "kernel/files/rootfs.img".to_string();
    let rootfs_img = std::path::Path::new(&rootfs_img_owned);

    let loader_img_owned = "kernel/files/loader.img".to_string();
    let loader_img = std::path::Path::new(&loader_img_owned);

    let dtb_img_owned = format!("{}/qemu.dtb", resolved.root.project.dist_dir());
    let dtb_img = std::path::Path::new(&dtb_img_owned);

    for p in [boot_bin, kernel_bin, rootfs_img, loader_img, dtb_img] {
        if !p.exists() {
            return Err(format!(
                "missing required artifact: {} (run `xtask code build` first)",
                p.display()
            ));
        }
    }

    println!("[xtask test] Booting CapsuleOS for ~{}s…", timeout_secs);
    let disk_img_str = crate::run::ensure_disk_image(&plat.qemu_disk_img)?;
    let mut cmd = Command::new(&plat.qemu_bin);
    for arg in &plat.qemu_args {
        let rendered = render_template_for_test(
            arg,
            plat,
            boot_bin.to_str().unwrap(),
            kernel_bin.to_str().unwrap(),
            rootfs_img.to_str().unwrap(),
            loader_img.to_str().unwrap(),
            dtb_img.to_str().unwrap(),
            &disk_img_str,
        );
        cmd.arg(rendered);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("failed to launch qemu: {}", e)),
    };

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = mpsc::channel::<String>();

    spawn_pump("qemu-stdout", BufReader::new(stdout), tx.clone());
    spawn_pump("qemu-stderr", BufReader::new(stderr), tx.clone());

    let started = Instant::now();
    let mut report_lines: Vec<String> = Vec::new();

    let mut pass_count: u32 = 0;
    let mut fail_count: u32 = 0;
    let mut total: u32 = 0;
    let mut summary_seen = false;

    let collect_timeout = Duration::from_secs(timeout_secs);
    let min_expected_passes: u32 = 105;

    while let Ok(line) = rx.recv_timeout(collect_timeout) {
        report_lines.push(line.clone());

        let normalised: String = line
            .chars()
            .skip_while(|c| matches!(*c, '\0' | '\u{1b}' | ' ' | '\r'))
            .collect();

        if let Some(_rest) = normalised.strip_prefix("[PASS] ") {
            pass_count += 1;
        } else if normalised.starts_with("[FAIL] ") {
            fail_count += 1;
        } else if let Some(rest) = normalised.strip_suffix(" passed") {
            if let Some(slash) = rest.find('/') {
                if let (Ok(seen), Ok(out_of)) = (
                    rest[..slash].parse::<u32>(),
                    rest[slash + 1..].trim().parse::<u32>(),
                ) {
                    if seen == out_of {
                        total = out_of;
                        summary_seen = true;
                        break;
                    }
                }
            }
        }

        if started.elapsed() >= collect_timeout {
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    let report = TestReport {
        passed: pass_count,
        failed: fail_count,
        total,
        log_tail: report_lines
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    };

    println!(
        "\n[xtask test] Captured: {} [PASS], {} [FAIL]",
        report.passed, report.failed
    );

    if !summary_seen {
        println!(
            "[xtask test] No `N/N passed` summary seen within {}s.",
            timeout_secs
        );
        println!("---- last 20 lines ----");
        for l in &report.log_tail {
            println!("    {}", l);
        }
        return Err(
            "test suite did not complete before timeout; inspect log tail above.".to_string(),
        );
    }

    if report.failed != 0 {
        println!("---- failures ----");
        for l in &report.log_tail {
            if l.starts_with("[FAIL] ") {
                println!("    {}", l);
            }
        }
        return Err(format!("{} test(s) failed.", report.failed));
    }

    if report.passed < min_expected_passes {
        return Err(format!(
            "too few tests run: expected >= {} but only saw {}",
            min_expected_passes, report.passed
        ));
    }

    println!(
        "[xtask test] SUCCESS: All {} tests passed successfully!",
        report.total
    );
    Ok(())
}

fn spawn_pump<R: Read + Send + 'static>(
    name: &'static str,
    mut reader: BufReader<R>,
    tx: mpsc::Sender<String>,
) {
    let _ = name;
    thread::spawn(move || {
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            let _ = tx.send(line.clone());
            line.clear();
        }
    });
}

fn render_template_for_test(
    template: &str,
    plat: &Platform,
    boot_bin: &str,
    kernel_bin: &str,
    rootfs_img: &str,
    loader_img: &str,
    dtb_img: &str,
    disk_img: &str,
) -> String {
    template
        .replace("{rust_target}", &plat.rust_target)
        .replace("{boot_addr}", &plat.boot_addr)
        .replace("{ohc_addr}", &plat.ohc_addr)
        .replace("{dtb_addr}", &plat.dtb_addr)
        .replace("{rootfs_addr}", &plat.rootfs_addr)
        .replace("{services_addr}", "0x48000000")
        .replace("{bootloader_bin}", boot_bin)
        .replace("{kernel_bin}", kernel_bin)
        .replace("{loader_img}", loader_img)
        .replace("{rootfs_img}", rootfs_img)
        .replace("{qemu_dtb}", dtb_img)
        .replace("{disk_img}", disk_img)
        .replace("{smp}", &plat.qemu_smp.to_string())
}
