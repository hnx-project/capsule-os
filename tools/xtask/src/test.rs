//! `xtask code test` — boots CapsuleOS under QEMU, captures the
//! `testall` console output, and asserts every CHECK expected by
//! the kernel/userspace smoke suite.
//!
//! Implementation is intentionally minimal: it shells `qemu-system-aarch64`,
//! scrapes stdout/stderr for the lines emitted by `testall` (one
//! `[PASS] <name>` per check and one final `N/N passed` summary), and
//! returns with a non-zero exit code if any check is `[FAIL]` or the
//! summary line doesn't appear within `timeout_secs` seconds.

use std::io::{BufRead, BufReader};
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
    // Make sure everything is built; the user typically does this
    // out of band, but doing it from `test` keeps the door closed.
    if let Err(e) = crate::build::build(resolved, plat, false) {
        return Err(format!("prebuild failed: {}", e));
    }

    let boot_bin_owned = format!("{}/aarch64-unknown-none/release/capsule-bootloader.bin", resolved.root.project.target_dir);
    let boot_bin = std::path::Path::new(&boot_bin_owned);

    let kernel_bin_owned = format!("{}/kernel/hnxcore", resolved.root.project.dist_dir());
    let kernel_bin = std::path::Path::new(&kernel_bin_owned);

    let rootfs_img_owned = resolved.build.subprojects.iter()
        .find(|sub| sub.subproject_type == "userspace")
        .and_then(|sub| sub.rootfs_output.as_ref())
        .cloned()
        .unwrap_or_else(|| "kernel/files/rootfs.img".to_string());
    let rootfs_img = std::path::Path::new(&rootfs_img_owned);

    let dtb_img_owned = format!("{}/qemu.dtb", resolved.root.project.dist_dir());
    let dtb_img = std::path::Path::new(&dtb_img_owned);

    for p in [boot_bin, kernel_bin, rootfs_img, dtb_img] {
        if !p.exists() {
            return Err(format!(
                "missing required artifact: {} (run `xtask code build` first)",
                p.display()
            ));
        }
    }

    println!("[xtask test] Booting CapsuleOS for ~{}s…", timeout_secs);
    let mut cmd = Command::new(&plat.qemu_bin);
    for arg in &plat.qemu_args {
        let rendered = render_template_for_test(
            arg,
            plat,
            boot_bin.to_str().unwrap(),
            kernel_bin.to_str().unwrap(),
            rootfs_img.to_str().unwrap(),
            dtb_img.to_str().unwrap(),
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
    // After Tier A + Tier B + Tier C (signal roundtrip + WNOHANG
    // subtests in s13.rs) the testall suite emits 106 `[PASS]`
    // lines.  `min_expected_passes` is the **regression floor**:
    // the count must reach this value to consider the run a pass.
    // The actual count is captured by the `N/N passed` summary
    // line further down — that drives the primary pass/fail
    // decision; this constant only acts as the floor.
    //
    // To accept this commit's baseline: re-run
    //   `xtask code test --arch aarch64 --timeout 90`
    // and confirm the count remains 106/106 before tightening
    // further.
    let min_expected_passes: u32 = 105;

    while let Ok(line) = rx.recv_timeout(collect_timeout) {
        report_lines.push(line.clone());

        // Normalise away the leading NUL/ANSI/box characters
        // CapsuleOS sometimes glues on in front of [PASS], so
        // the prefix match is robust.
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
                // Skip empty sentinel so the main loop doesn't
                // spin on EOF-only empty lines.
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

// Convenience re-export kept narrow so other modules don't pull
// in the entire `run` namespace.
pub fn render_template_for_test(
    template: &str,
    plat: &Platform,
    bootloader_bin: &str,
    kernel_bin: &str,
    rootfs_img: &str,
    dtb_img: &str,
) -> String {
    template
        .replace("{rust_target}", &plat.rust_target)
        .replace("{boot_addr}", &plat.boot_addr)
        .replace("{ohc_addr}", &plat.ohc_addr)
        .replace("{dtb_addr}", &plat.dtb_addr)
        .replace("{rootfs_addr}", &plat.rootfs_addr)
        .replace("{bootloader_bin}", bootloader_bin)
        .replace(
            "{bootloader_bin_raw}",
            bootloader_bin.trim_end_matches(".bin"),
        )
        .replace("{kernel_bin}", kernel_bin)
        .replace("{rootfs_img}", rootfs_img)
        .replace("{qemu_dtb}", dtb_img)
        .replace("{smp}", &plat.qemu_smp.to_string())
}

#[allow(dead_code)]
pub fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

// Ensure the Config shape is referenced even though we don't read
// any field from it yet — future S0.2 (version matrix checks, etc.)
// will.
#[allow(dead_code)]
fn _force_config_link(_: &crate::config::RootConfig) {}

#[cfg(test)]
mod tests {
    //! Lightweight smoke tests that exercise xtask's template
    //! renderer without spawning QEMU.  These run with the
    //! ordinary `cargo test --manifest-path tools/xtask/Cargo.toml`
    //! invocation and double as a contract for any future
    //! changes to the `qemu_args` / `dtb_dump_args` placeholder
    //! grammar.
    use super::render_template_for_test;
    use crate::platform::Platform;

    fn fake_platform(smp: u32) -> Platform {
        Platform {
            arch: "aarch64".to_string(),
            profile: "virt".to_string(),
            rust_target: "aarch64-unknown-none".to_string(),
            userspace_target: "x".to_string(),
            kernel_entry: "0".to_string(),
            ld_emulation: "aarch64elf".to_string(),
            linker_script: "x".to_string(),
            dtb_addr: "0x42000000".to_string(),
            ohc_addr: "0x40700000".to_string(),
            boot_addr: "0x44000000".to_string(),
            rootfs_addr: "0x46000000".to_string(),
            qemu_bin: "qemu-system-aarch64".to_string(),
            qemu_args: vec!["-smp".into(), "{smp}".into()],
            qemu_dtb_dump_args: vec![
                "-dumpdtb=/tmp/r.dtb".into(),
                "-smp".into(),
                "{smp}".into(),
            ],
            qemu_smp: smp,
        }
    }

    #[test]
    fn smp_placeholder_substitution() {
        let p = fake_platform(4);
        let rendered = render_template_for_test(
            "{smp}", &p, "/b", "/k", "/r", "/d",
        );
        assert_eq!(rendered, "4");
    }

    #[test]
    fn multi_template_substitution() {
        let p = fake_platform(2);
        let s = render_template_for_test(
            "-smp {smp} -kernel {kernel_bin}",
            &p, "/b", "/tmp/hnxcore", "/r", "/d",
        );
        assert!(s.contains("-smp 2"));
        assert!(s.contains("-kernel /tmp/hnxcore"));
    }

    #[test]
    fn zero_smp_replaces_with_zero() {
        let p = fake_platform(0);
        let rendered = render_template_for_test("{smp}", &p, "/b", "/k", "/r", "/d");
        assert_eq!(rendered, "0");
    }

    #[test]
    fn unknown_placeholder_passes_through() {
        let p = fake_platform(1);
        let s = render_template_for_test(
            "--fake={nope}", &p, "/b", "/k", "/r", "/d",
        );
        assert_eq!(s, "--fake={nope}");
    }
}
