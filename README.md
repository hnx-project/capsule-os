# 🌌 CapsuleOS: A Pure-Rust Microkernel Operating System

<div align="center">
  <img src="https://img.shields.io/badge/OS-CapsuleOS-6f42c1?style=for-the-badge&logo=rust" alt="CapsuleOS" />
  <img src="https://img.shields.io/badge/Architecture-AArch64-success?style=for-the-badge" alt="Architecture" />
  <img src="https://img.shields.io/badge/Version-Pangu%201.0.0.beta-blue?style=for-the-badge" alt="Current Version" />
</div>

---

**CapsuleOS** is a next-generation, high-performance microkernel operating system built entirely from scratch in pure **Rust** for the **AArch64** architecture. 

It isolates traditional OS subsystems (virtual file systems, device drivers, and loaders) into secure user-space EL0 sandboxes, completely decoupled from standard ELF loading overheads via our lightweight **OHLINK binary standard**.

---

## 📖 Documentation Index

To maintain professional, decoupled documentation, the repository is organized into specialized specifications:

*   **[DEVELOPMENT.md](./DEVELOPMENT.md)** — **Engineering Standards**: Comprehensive guide detailing the 8 Core Engineering Standards (Architecture abstraction, Kernel logic, Syscall contracts, `libcapsule` interfaces, `libc` translation, self-built `libstd`, Sandboxed services, and EL0 programs).
*   **[ARCHITECTURE.md](./ARCHITECTURE.md)** — **Core Design Specs**: Deep-dive into L0–L4 layered architecture, the zero-trust Capability security model, MMU-enforced address spaces, and Synchronous Rendezvous IPC.
*   **[CONTRIBUTING.md](./CONTRIBUTING.md)** — **Developer Workflows**: Guide to environment setup, build-test cycles, conventional commit rules, and merging code.
*   **[AGENTS.md](./AGENTS.md)** — **AI Agent Guidelines**: Operational specifications, safety mandates, and repository invariants for autonomous AI collaborators.
*   **[CHANGELOG.md](./CHANGELOG.md)** — **Release History**: Version-by-version audit trail formatted under Keep a Changelog.

---

## 🛠️ Quick Start

Bring up a working CapsuleOS session in QEMU from a fresh clone:

```bash
# 1. Bootstrap the companion toolchain (adds xtask to your shell environment)
./install_xtask

# 2. Build the entire OS, bootloader, and user-space binaries
xtask code build --arch aarch64

# 3. Run and verify inside the QEMU Emulator
xtask code run --arch aarch64
```

*Note: Use `xtask code check-env` to diagnose host compiler alignment issues.*

---

## 📊 Project Status & Verification

Current Version: **Pangu 1.0.0.beta**

Kernel stability, IPC channel integrity, and POSIX-compatible filesystem APIs are verified end-to-end via our custom **`testall`** program running in the user-space sandbox:

```text
===== All Test Suite =====
[PASS] connect
[PASS] create_file
[PASS] read_file
[PASS] mkdir
[PASS] mkdir_dup
[PASS] readdir
[PASS] stat
[PASS] unlink
[PASS] rmdir
[PASS] rmdir_nonempty
[PASS] stress_vfs
[PASS] peer_close
12/12 passed
```

---

## 📜 License

CapsuleOS is released under the **Apache License, Version 2.0**. See [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE) for full terms and third-party subtree attributions.

---
*Designed and engineered with passion by the **HNX-Project** community.*
