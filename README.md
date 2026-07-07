# 🌌 CapsuleOS: A Pure-Rust Microkernel Operating System

<div align="center">
  <img src="https://img.shields.io/badge/OS-CapsuleOS-6f42c1?style=for-the-badge&logo=rust" alt="CapsuleOS" />
  <img src="https://img.shields.io/badge/Architecture-AArch64%20%7C%20RISCV64-success?style=for-the-badge" alt="Architecture" />
  <img src="https://img.shields.io/badge/Kernel-HNX%20v0.3.1-blue?style=for-the-badge" alt="HNX Kernel" />
  <img src="https://img.shields.io/badge/Format-OHLINK-orange?style=for-the-badge" alt="OHLINK Format" />
</div>

---

**CapsuleOS** (Codename: **Pangu / 开天辟地**) is a next-generation, high-performance microkernel operating system built entirely from scratch in pure **Rust**. Designed with modularity, zero-trust security, and high performance in mind, CapsuleOS pushes the boundaries of modern bare-metal system programming by shifting traditional OS components (such as device drivers, virtual file systems, and dynamic loaders) completely into secure user-space sandboxes.

Our ecosystem operates completely decoupled from traditional heavyweight binary formats like GNU ELF, relying instead on our ultra-lightweight, custom-designed **OHLINK binary format**, complete with pure-Rust static linking, symbol resolution, and high-performance instruction relocations.

---

## 🚀 Key Highlights & Innovations

### 🛡️ 1. Pure Rust Microkernel Architecture (HNX)
At the heart of CapsuleOS lies the **HNX Microkernel**, refined to a tiny **30KB binary runtime footprint**. By employing link-time optimization (LTO) and strict code-dead-stripping (`--gc-sections`), HNX implements only the absolute minimal primitives in privileged Mode (EL1/S-Mode):
* **Thread Scheduling**: Preemptive, high-frequency tick scheduling.
* **Capabilities & Handles**: Zero-trust, handle-based resource encapsulation and delegation modeled after seL4 and Zircon.
* **Virtual Memory**: Advanced VM objects (VMO) and Address Regions (VMAR) with fine-grained page tables.
* **Ultra-Fast IPC**: Zero-allocation Synchronous Rendezvous Channels with **Direct Handoff** capabilities, enabling zero-copy transfer of kernel Capabilities across isolated process boundaries.

### 📦 2. The Custom OHLINK Binary Standard
CapsuleOS rejects the heavy overhead of loading complex ELF binaries in sandboxed environments. We developed the **OHLINK specification**, a custom bare-metal executable and dynamic linking structure:
* **Pure Rust Toolchain (`ohlink-cc`)**: A dynamic dynamic-library `rustc_codegen_ohlink` backend paired with `ohlink-linker` supporting 6 major AArch64 relocation equations (including `R_AARCH64_ABS64`, `R_AARCH64_CALL26`, and `R_AARCH64_ADR_PREL_PG_HI21`).
* **Microkernel Safe Loader**: A highly secure `#![no_std]` OHLINK parser built into the microkernel and bootloader, incorporating robust CRC32-IEEE checksum validation and automatic segment page-alignment.
* **Format-Level Isolation**: Complete separation of executable sections (`.text` is marked as Read-Only + Execute, `.rodata` as Read-Only + Non-Executable, and `.data` as Read-Write + Non-Executable) enforced securely via 4-level MMU hardware translations.

### 🧱 3. Comprehensive User-Space Sandbox & `hnxstd`
To make writing secure OS services highly developer-friendly, CapsuleOS features:
* **Custom Target Spec**: Official target triples `aarch64-unknown-capsule` and `riscv64-unknown-capsule` that define the OS environment.
* **`hnxlibc` ABI**: A clean layer mapping standard C-ABI symbols (`write`, `read`, `exit`, etc.) to low-level microkernel system calls.
* **`hnxstd` Standard Library**: A self-built, fully compliant standard library providing `Vec`, `String`, `println!`, and core collections to sandboxed user-space servers like `init`, `loader`, `devmgr`, and `vfs`.

---

## 📂 Unified Monorepo Layout (Subtree-Driven)

CapsuleOS has transitioned from complex, fragile Git Submodules into a highly robust **Git Subtree Monorepo** architecture. This ensures that any developer can clone a single repository and compile the entire operating system, firmwares, and tools instantly with zero external key or authentication issues.

```text
.
├── bootloader/            # 📂 (Subtree: capsule-bootloader) Arm64 Bare-Metal Bootloader
├── kernel/                # 📂 (Subtree: hnx-core) Privileged Microkernel Runtime
│   ├── linker/            # 📜 Architecture linker scripts
│   └── src/               # 🦀 MMU, Scheduler, Interrupts, and System Calls
├── userspace/             # 📂 User-Space Sandboxed Ecosystem
│   ├── hnxlibc/           # 🧬 Standard C system-call bridging library
│   ├── hnxstd/            # 🦀 Self-built Rust standard library (Vec, String, Println)
│   ├── services/          # 🛡️ Sandboxed servers (init, devmgr, vfs, loader)
│   └── programs/          # 🐚 Shell, CLI applications, and tools
├── std/                   # 📂 Cross-Compilation Spec Definitions
│   └── targets/           # 📜 JSON target specifications for Rustc
├── tools/                 # 📂 System Build and Packaging Tools
│   ├── ohlink-cc/         # 📂 (Subtree: ohlink-cc) Pure-Rust Compiler Backend, Linker, and VM Emulator
│   └── xtask/             # 🎛️ Dual-Star build orchestrator and GitCode manager
└── xtask.toml             # 📜 Global build and project metadata
```

---

## 🛠️ The Dual-Star `xtask` Dev Workflow

CapsuleOS comes with a self-bootstrapping developer companion toolchain (`xtask`) designed to manage the entire OS development life cycle and upstream GitCode integration.

To install the `xtask` binary globally on your host, simply execute the bootstrap script:
```bash
./install_xtask
```

### 💻 1. The Code Subcommand Suite (`xtask code`)
Focuses on compiling, compiling, and executing the microkernel ecosystem:

```bash
# 🖥️ Build the entire OS, userspace, and bootloader for aarch64
xtask code build --arch aarch64

# 🚀 Compile and launch CapsuleOS instantly in AArch64 QEMU Emulator
xtask code run --arch aarch64

# 🔬 Check local host cross-compilation toolchain and environment
xtask code check-env
```

### 🔏 2. The Repo Subcommand Suite (`xtask repo`)
Enforces strict open-source collaboration guidelines, automating subtree alignment, conventional commits, and release management:

```bash
# ⚙️ Initialize fork topology and setup tracking for local development
xtask repo setup-fork --username <YourGitCodeUsername>

# 🧼 Lint, check targets for zero-warnings, and create standard Conventional Commits
xtask repo commit --type feat --scope loader --message "add ohlink loader"

# 📥 Pull and synchronize all Git subtrees with upstream squash integrations
xtask repo pull

# 📤 Verify and push changes recursively across workspace to personal Fork
xtask repo push

# 📦 (Admin Only) Compile, package into standard releases, and upload to GitCode via API
xtask repo release v0.5.6
```

---

## 🖥️ Booting Output Demonstration
When launched using `xtask code run --arch aarch64`, the bootloader aligns, validates, and hands control over to the HNX Microkernel, which securely initiates the sandboxed EL0 environment:

```text
[  INFO ] [BOOT  ] Booting v0.5.6-develop...
[  INFO ] [BOOT  ] DTB found at fallback addr 0x42000000.
[  INFO ] [BOOT  ] Valid OHC Image Found!
[  INFO ] [BOOT  ] => Version : 0x0002
[  INFO ] [BOOT  ] => Entry   : 0x0000000040080000
[  INFO ] [BOOT  ] => Segments: 1
[  INFO ] [BOOT  ] => Size    : 0x0002ec58 bytes
[  INFO ] [BOOT  ] Extracting payload to entry point...
[  INFO ] [BOOT  ] Jumping to HNX Kernel...

[  INFO ] [KERNEL] HNX v0.3.1
[  INFO ] [FDT   ] Discovered hardware:
[  INFO ] [FDT   ] => UART base : 0x9000000
[  INFO ] [FDT   ] => RAM base  : 0x40000000
[  INFO ] [FDT   ] => RAM size  : 0x20000000 (512 MB)
[  INFO ] [MM    ] Physical page allocator initialized.
[  INFO ] [MMU   ] 4-level page tables ACTIVE
[  INFO ] [IRQ   ] GIC + generic timer enabled
[  INFO ] [BOOT  ] OK
[  INFO ] [SMOKE ] VMO/VMAR smoke test: MATCH (via MMU translation)
[  INFO ] [TASK  ] init & worker threads created with process handle tables
[  INFO ] [LOADER] Loader service launched successfully at EL0!
[  INFO ] [BOOT  ] Loader process ready to schedule
[  INFO ] [HANDLE] smoke test PASS
[  INFO ] [SCHED ] Starting preemptive multitasking...
```

---

## 🛡️ Security Architecture & Principles
* **Separation of Concerns**: Microkernel execution is strictly restricted to memory management, scheduling, capability policing, and thread contexts.
* **Capability-Based Authorization**: Processes cannot reference raw addresses or system objects directly. Every action is gated by a `Handle` with fine-grained permission bits (e.g. `READ`, `WRITE`, `EXECUTE`, `MAP`).
* **Zero-Allocation IPC**: Channels and Ports are engineered using page-preallocated completion pools and Thread-Control-Block (TCB) inline queues. This removes the possibility of Kernel-Heap-Exhaustion attacks, which often cause system-wide panics in other OS implementations.

---

## 🤝 Contributing to CapsuleOS
CapsuleOS is an actively maintained open-source system. If you want to contribute:
1. Fork the upstream repository.
2. Initialize with `./install_xtask` and `xtask repo setup-fork`.
3. Commit with `xtask repo commit` to ensure code formatting, target compilation, and strict lint checks pass perfectly.
4. Push with `xtask repo push` and open a Merge Request targeting the upstream `develop` branch!

*Designed and engineered with passion by **TinchyChin** and the **HNX-Project** community.*
