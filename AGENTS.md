# CapsuleOS - AI Agent Guidelines & Engineering Specifications

This document outlines the strict engineering conventions, layout definitions, and automated workflow rules for AI Agents (including `opencode` and other subagents) when operating inside the **CapsuleOS** codebase.

---

## 🌌 System Overview
**CapsuleOS** (Codename: **Pangu**) is a from-scratch microkernel operating system built entirely in Rust.
*   **Privileged Level**: HNX Microkernel (EL1 on AArch64).
*   **Target Platform**: `aarch64-unknown-none` (soft-float ABI).
*   **User-Space Triple**: `aarch64-unknown-capsule`.
*   **Standard Runtime**: Self-built `#![no_std]` `libstd` standard library bridged through `libc`.

---

## 🛠️ Unified Binary Standard: OHLINK
CapsuleOS utilizes a custom **OHLINK format** across userspace and bootloaders, decoupling the operating system completely from standard ELF loading overheads.

### 📜 Header Structure (`OHLK_Header`) - 48 Bytes
Every valid OHLINK binary must begin with a 48-byte header mapping:
*   **Magic (4B)**: `0x4F484C4B` ("OHLK").
*   **File Size (8B)**: Complete size in bytes.
*   **Checksum (4B)**: Standard CRC32-IEEE checksum of the entire file (with the checksum field zeroed out during calculation).
*   **Header Count (2B)**: Number of header table descriptor entries ($N$).

### 🧬 Entry Table (`OHLK_Entry`) - 32 Bytes
Following the header is a table of $N \times 32$-byte descriptors. Loadable sections must be flagged appropriately:
*   **`TYPE_TEXT` (`0x00010001`)**: Read-Only + Execute (RX) code segments.
*   **`TYPE_DATA` (`0x00020002`)**: Read-Write + Non-Executable (RW, NX) initialized data.
*   **`TYPE_RODATA` (`0x00030003`)**: Read-Only + Non-Executable (RO, NX) constants.
*   **`TYPE_BSS` (`0x00040004`)**: Uninitialized global variables (`file_size` = 0, `mem_size` > 0).

---

## 📦 Unified Subtree Monorepo Architecture

All projects are integrated using **Git Subtrees** under a single repository structure. AI Agents must respect the workspace definitions and treat all subtrees as local directories.

### 📁 Workspace Folder Definitions
```text
.
├── bootloader/            # 📂 (Subtree) capsule-bootloader source (L0)
├── kernel/                # 📂 (Subtree) hnx-core source (L1)
│   └── src/               # Process/Thread context, VMAR, VMO, IPC Channels, Scheduler, HAL
├── libraries/             # 📂 Top-level OS libraries (libc, libstd, libcapsule) (L2)
│   ├── libc/              # 🧬 Standard C system-call bridging library
│   ├── libstd/            # 🦀 Self-built Rust standard library (Vec, String, Println)
│   ├── libcapsule/        # 📂 System specialized helper library
│   └── targets/           # CapsuleOS custom cross-compilation JSON targets
├── userspace/             # 📂 User-Space Sandbox Ecosystem (services + programs)
│   └── services/          # Sandboxed system services (devmgr, fileagent) (L3)
└── tools/                 # 📂 Development Tooling
    └── ohlink-cc/         # 📂 (Subtree) ohlink-format, rustc_codegen_ohlink, ohlink-linker
```

The **`libc/` and `libstd/`** crates are top-level OS runtime contract crates — they are **not** EL0 sandbox services. Both are inherited by downstream manifests via `dep.workspace = true` (see `[workspace.dependencies]` in the root `Cargo.toml`).

---

## 🤖 AI Agent Workflow Rules

To ensure a seamless, non-breaking developer experience and zero merge conflicts, all AI Agents must adhere to the following workflow loop:

### 1. 🔒 Local Git Identity Check
Before staging or committing any code, ensure the repository `user.name` and `user.email` are correctly set to your development identity. Do not commit with anonymous or placeholder emails.

### 2. 💻 Compilation and Execution
Never invoke raw custom linker calls. Always route compilation and emulation through the `xtask` orchestrator tool:

```bash
# Compile host ohlink tools and cross-compile AArch64 targets
xtask code build --arch aarch64

# Run and verify inside QEMU Emulator
xtask code run --arch aarch64
```

### 3. 🛡️ Safety & Quality Verification
*   **Adherence to DEVELOPMENT.md**: The AI Agent must strictly verify all code changes against the **8 Core Development Standards** defined in **[DEVELOPMENT.md](./DEVELOPMENT.md)**.
*   **Warnings Mitigation**: Any warning generated during `xtask code build` (including unused imports or variables) should be solved proactively prior to merging.
*   **Handle Isolation**: Never pass raw physical/virtual pointers across user-space system calls. Use capability `HandleValue` mappings securely managed under `HandleTable`.

### 4. 🔇 Debug Output Discipline
**Keep debug logging as terse as possible.** Every print call in the kernel and the userspace service tier degrades performance. Default to *silent*; only break the silence when troubleshooting a specific bug, and **delete the logs once the bug is resolved**.

*   **One line per event**: Never log a multi-line block in hot execution paths (alloc loops, scheduler traces).
*   **No trace-and-keep**: If a `log_info!` or `kprintln!` was added to track a bug, delete it once resolved.
*   **Structured, not narrative**: Use concise, structured log events rather than verbose narrative strings.

---
*Prepared by **TinchyChin** and the **HNX-Project** administrator group.*
