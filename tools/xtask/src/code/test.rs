use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Resolved;
use crate::platform::Platform;

#[derive(Debug)]
pub struct TestReport {
    pub passed: u32,
    pub failed: u32,
    pub total: u32,
    pub log_tail: Vec<String>,
}

pub fn test(resolved: &Resolved, plat: &Platform, timeout_secs: u64) -> Result<(), String> {
    if let Err(e) = crate::code::build::build(resolved, plat, false) {
        return Err(format!("prebuild failed: {}", e));
    }

    // Find host standard UEFI BIOS firmware
    let uefi_bios_path = crate::code::run::find_uefi_bios_path()?;

    let boot_efi_owned = format!("{}/EFI/BOOT/BOOTAA64.EFI", crate::config::BUILD_TEMP_RESOURCE);
    let boot_efi = std::path::Path::new(&boot_efi_owned);

    if !boot_efi.exists() {
        return Err(format!(
            "missing required UEFI bootloader: {} (run `xtask code build` first)",
            boot_efi.display()
        ));
    }

    println!("[xtask test] Booting CapsuleOS for ~{}s…", timeout_secs);
    let mut cmd = Command::new(&plat.qemu_bin);
    for arg in &plat.qemu_args {
        let rendered = arg
            .replace("{uefi_bios}", &uefi_bios_path)
            .replace("{disk_dir}", crate::config::BUILD_TEMP_RESOURCE)
            .replace("{smp}", &plat.qemu_smp.to_string());
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
            "only {} [PASS] lines seen (expected at least {}). \
             The test suite likely regressed; restore `testall` first.",
            report.passed, min_expected_passes,
        ));
    }

    println!(
        "[xtask test] OK: {}/{} checks passed (>= {} baseline).",
        report.passed, report.total, min_expected_passes
    );
    Ok(())
}

fn spawn_pump<R: std::io::Read + Send + 'static>(
    name: &'static str,
    reader: R,
    tx: mpsc::Sender<String>,
) {
    thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let mut buf = String::new();
            let mut reader = BufReader::new(reader);
            loop {
                buf.clear();
                let n = match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                if n == 0 {
                    break;
                }
                let trimmed = buf.trim_end_matches(['\n', '\r']);
                if trimmed.is_empty() {
                    continue;
                }
                if tx.send(trimmed.to_string()).is_err() {
                    break;
                }
            }
        })
        .expect("failed to spawn qemu pump thread");
}
