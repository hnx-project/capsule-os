# xtask (CapsuleOS Build Orchestrator) — API

## Status
`Active (使用中)`

## Name
`tools/xtask` — the unified Rust-based build orchestrator. Wraps cargo,
`rust-lld`, `llvm-objcopy`, `ohlink-*` and QEMU into the four command
families: `clean`, `code build`, `code run`, `code test`.

## Dependencies & Related Components
- **Cargo workspace** at repository root (`Cargo.toml`).
- **Cross toolchain** described by `libraries/targets/aarch64-unknown-capsule.json`.
- **OHLINK toolchain** in `tools/ohlink-toolchain` (host-side).
- **QEMU** (`qemu-system-aarch64`) for `code run --platform virt`.
- **TOML configs**:
  - `xtask.toml` — root metadata, gitcode, toolchain, distribution template.
  - `xtask.build.toml` — per-arch/per-platform build addresses and
    `[[subprojects]]` table.
  - `xtask.qemu.toml` — QEMU runtime profile (consumed by `code run/test --platform virt`).
  - `xtask.rpi.toml` — RPI runtime profile (consumed by `--platform rpi`).

## Core Definition
xtask is the single entry point that turns source trees into a
bootable CapsuleOS image, drives QEMU, and validates the `testall`
suite inside QEMU.

## Exposed Interfaces

### CLI surface (`xtask code …`)
| Command | Description |
|---------|-------------|
| `xtask clean` | Wipe build artifacts (`build/`, `target/`). |
| `xtask code build [--arch aarch64] [--platform virt|rpi] [--config <path>]` | Compile bootloader, kernel, libc, libstd, libcapsule and userspace. With `platform virt`, regenerates `build/dist/qemu.dtb`. |
| `xtask code run [--arch aarch64] [--platform virt|rpi] [--gdb] [--config <path>]` | Auto-clean + auto-build + launch QEMU. |
| `xtask code test [--arch aarch64] [--platform virt|rpi] [--timeout <secs>] [--config <path>]` | Auto-build + boot QEMU + scrape `testall` PASS/FAIL summary. |
| `xtask code check-env` | Verify cross-toolchain and host QEMU availability. |
| `xtask code check-version [--sync]` | Compare `xtask.toml`/`Cargo.toml` versions across the workspace. |
| `xtask code doc [--open]` | Generate rustdoc. |

### `xtask.qemu.toml` QEMU block (`[platform.<arch>.profiles.<name>.qemu]`)
| Field | Type | Default | Meaning |
|-------|------|---------|---------|
| `bin` | string | required | QEMU binary, e.g. `qemu-system-aarch64`. |
| `args` | array of strings | required | QEMU args. Supports placeholders: `{rust_target}`, `{boot_addr}`, `{ohc_addr}`, `{dtb_addr}`, `{rootfs_addr}`, `{services_addr}`, `{bootloader_bin}`, `{bootloader_bin_raw}`, `{kernel_bin}`, `{loader_img}`, `{rootfs_img}`, `{qemu_dtb}`, `{disk_img}`, `{smp}`. |
| `dtb_dump_args` | array of strings | required | Args for the pre-launch `dumpdtb` invocation (used to harvest QEMU's device tree into `build/dist/qemu.dtb`). |
| `smp` | integer | `1` | Guest CPU count. Templated as `{smp}`. |
| `disk_img` | string | `build/disk.img` | Path to the virtio block image referenced by `-drive file=…`. xtask synthesises a 1.44 MB FAT12 stub at this path before launching QEMU if the file is missing, so the run no longer requires a stale `disk.img` in the repository root. The resolved path is templated into `args` as `{disk_img}`. |

### Template placeholder grammar
```
{disk_img}           -> absolute path to the virtio block image (default: build/disk.img)
{bootloader_bin}     -> path to capsule-bootloader.bin
{bootloader_bin_raw} -> same path minus the .bin suffix
{kernel_bin}         -> path to hnxcore (kernel OHLC image)
{loader_img}         -> path to userspace-loader rootfs image
{rootfs_img}         -> path to services rootfs image (alias for "services" VMAR)
{qemu_dtb}           -> path to the freshly-dumped build/dist/qemu.dtb
{smp}                -> qemu_smp value
{boot_addr}/{ohc_addr}/{dtb_addr}/{rootfs_addr}/{services_addr} -> hex addresses from xtask.build.toml
{rust_target}        -> Rust target triple from xtask.build.toml
```

### Helpers exposed to other modules
- `run::resolve_artifact_paths(&BuildConfig, &Platform) -> (bootloader_bin, bootloader_bin_raw, kernel_bin, loader_img, services_img)`
- `run::generate_qemu_dtb(&Resolved, &Platform, ...)` — rebuild `build/dist/qemu.dtb` via `dumpdtb`.
- `run::generate_qemu_dtb_artifact_paths(&Resolved, &Platform)` — public hook called by `build` after a clean build.
- `run::ensure_disk_image(&str) -> String` — ensure the configured `disk_img` exists; synthesises a 1.44 MB FAT12 stub if missing; returns the canonical absolute path.
- `test::test(&Resolved, &Platform, timeout_secs)` — boots QEMU, scrapes `testall` output, returns PASS/FAIL counts.

### Behavioural contracts
- `xtask code run` will refuse to silently omit a virtio block image: it materialises `build/disk.img` (or whatever `disk_img` is configured to) before launching QEMU.
- QEMU stdout/stderr is inherited from the terminal. The caller decides capture (e.g. `xtask code run --arch aarch64 2>&1 | tee mylog.log`). xtask performs no implicit log capture, no auto-clean, and no auto-rebuild — `xtask code clean` and `xtask code build` are independent commands the caller invokes before `run` if needed.
