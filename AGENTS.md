# CapsuleOS - AI Agent Guidelines & Engineering Specifications

This document outlines the strict engineering conventions, layout definitions, and automated workflow rules for AI Agents (including opencode and other subagents) when operating inside the **CapsuleOS** codebase.

---

## 🌌 System Overview
**CapsuleOS** (代号: **Pangu**) is a from-scratch microkernel operating system built entirely in Rust.
* **Privileged Level**: HNX Microkernel (EL1 on AArch64, S-Mode on RISC-V 64).
* **Target Platforms**: `aarch64-unknown-none` & `riscv64imac-unknown-none-elf` (soft-float ABI).
* **User-Space Triples**: `aarch64-unknown-capsule` & `riscv64-unknown-capsule`.
* **Standard Runtime**: Self-built `#![no_std]` `hnxstd` standard library bridged through `hnxlibc`.

---

## 🛠️ Unified Binary Standard: OHLINK
CapsuleOS utilizes a custom **OHLINK format** across userspace and bootloaders, decoupling the operating system completely from standard ELF loading overheads.

### 📜 Header Structure (`OHLK_Header`) - 48 Bytes
Every valid OHLINK binary must begin with a 48-byte header mapping:
* **Magic (4B)**: `0x4F484C4B` ("OHLK").
* **File Size (8B)**: Complete size in bytes.
* **Checksum (4B)**: Standard CRC32-IEEE checksum of the entire file (with the checksum field zeroed out during calculation).
* **Header Count (2B)**: Number of header table descriptor entries ($N$).

### 🧬 Entry Table (`OHLK_Entry`) - 32 Bytes
Following the header is a table of $N \times 32$-byte descriptors. Loadable sections must be flagged appropriately:
* **`TYPE_TEXT` (`0x00010001`)**: Read-Only + Execute (RX) code segments.
* **`TYPE_DATA` (`0x00020002`)**: Read-Write + Non-Executable (RW, NX) initialized data.
* **`TYPE_RODATA` (`0x00030003`)**: Read-Only + Non-Executable (RO, NX) constants.
* **`TYPE_BSS` (`0x00040004`)**: Uninitialized global variables (`file_size` = 0, `mem_size` > 0).

---

## 📦 Unified Subtree Monorepo Architecture

AI Agents must respect that **all projects are now strictly integrated using Git Subtrees** rather than legacy Git Submodules.

### 📁 Workspace Folder Definitions
```text
.
├── bootloader/            # 📂 (Subtree) capsule-bootloader source
├── kernel/                # 📂 (Subtree) hnx-core source
│   ├── shared/            # Common shared kernel/userspace status and types
│   ├── hal/               # Hardware Abstraction Layer
│   └── src/               # Process/Thread context, VMAR, VMO, IPC Channels, Scheduler
├── userspace/             # 📂 User-Space Sandbox Ecosystem
│   ├── hnxlibc/           # Standard C-ABI syscall wrappers
│   ├── hnxstd/            # Pure-Rust custom standard library
│   └── services/          # Sandboxed system services (init, devmgr, loader, vfs)
├── tools/                 # 📂 Development Tooling
│   └── ohlink-cc/         # 📂 (Subtree) ohlink-format, rustc_codegen_ohlink, ohlink-linker
└── std/targets/           # 📜 CapsuleOS custom cross-compilation JSON targets
```

---

## 🤖 AI Agent Workflow Rules

To ensure a seamless, non-breaking developer experience and zero merge conflicts, all AI Agents must adhere to the following workflow loop:

### 1. 🔒 Local Git Identity Lock
Before staging or committing any code, you **MUST** ensure the local repository identity is locked to the official CapsuleOS administrator:
```bash
git config --local user.name "TinchyChin"
git config --local user.email "tinchychin97@gmail.com"
```

### 2. 💻 Use the `xtask` Command Suite Only
Never invoke raw compilation or raw git pushes. Always route instructions through the self-built **`xtask` Dual-Star** toolchain.

* **For Building and Running OS**:
  ```bash
  # Compile host ohlink tools and cross-compile targets
  cargo xtask code build --arch aarch64
  
  # Run and verify inside QEMU Emulator
  cargo xtask code run --arch aarch64
  ```
* **For Code Modification & Commit**:
  Always route your staging and commit hooks through the secure `xtask` repo pipeline:
  ```bash
  git add .
  cargo xtask repo commit --type "<TYPE>" --scope "<SCOPE>" --message "<DESCRIPTION>"
  ```
  *This automatically triggers our pre-commit safety grid: rustfmt check, workspace warnings scan, dual-architecture cross-compilation checks, and version overlap warnings.*

* **For Pushing modifications upstream**:
  Never push with `git push` directly. Always use:
  ```bash
  cargo xtask repo push
  ```
  *This ensures that all subtree tracking points, local branches, and clean squashed merges remain aligned with upstream.*

### 3. 🛡️ Verification Policies
* **Warnings as Errors**: Any warning generated during `cargo check` (including unused imports or variables) should be solved immediately before staging.
* **Architecture Agnosticism**: When modifying any core microkernel file (`kernel/src`), you must ensure both `aarch64` and `riscv64` targets compile flawlessly.
* **Handle Isolation**: Never pass raw physical/virtual pointers across user-space system calls. Use Zircon/seL4 capability `Handle` mappings securely managed under `HandleTable`.

---

*Prepared by **TinchyChin** and the **HNX-Project** administrator group.*
