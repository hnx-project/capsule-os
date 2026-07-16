# 🌌 CapsuleOS: A Pure-Rust Microkernel Operating System

<div align="center">
  <img src="https://img.shields.io/badge/OS-CapsuleOS-6f42c1?style=for-the-badge&logo=rust" alt="CapsuleOS" />
  <img src="https://img.shields.io/badge/Architecture-AArch64%20%7C%20RISCV64-success?style=for-the-badge" alt="Architecture" />
  <img src="https://img.shields.io/badge/Kernel-HNX%20v0.5.9-blue?style=for-the-badge" alt="HNX Kernel" />
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

### 🧱 3. Comprehensive User-Space Sandbox & `libstd`
To make writing secure OS services highly developer-friendly, CapsuleOS features:
* **Custom Target Spec**: Official target triples `aarch64-unknown-capsule` and `riscv64-unknown-capsule` that define the OS environment.
* **`libc` ABI**: A clean layer mapping standard C-ABI symbols (`write`, `read`, `exit`, etc.) to low-level microkernel system calls.
* **`libstd` Standard Library**: A self-built, fully compliant standard library providing `Vec`, `String`, `println!`, and core collections to sandboxed user-space servers like `init`, `loader`, `devmgr`, and `vfs`.

---

## 📂 Unified Monorepo Layout (Subtree-Driven)

CapsuleOS has transitioned from complex, fragile Git Submodules into a highly robust **Git Subtree Monorepo** architecture. This ensures that any developer can clone a single repository and compile the entire operating system, firmwares, and tools instantly with zero external key or authentication issues.

```text
.
├── bootloader/            # 📂 (Subtree: capsule-bootloader) Arm64 Bare-Metal Bootloader
├── kernel/                # 📂 (Subtree: hnx-core) Privileged Microkernel Runtime
│   ├── linker/            # 📜 Architecture linker scripts
│   └── src/               # 🦀 MMU, Scheduler, Interrupts, and System Calls
├── libraries/             # 📂 Top-level OS libraries (libc, libstd, libcapsule, targets)
│   ├── libc/              # 🧬 Standard C system-call bridging library
│   ├── libstd/            # 🦀 Self-built Rust standard library (Vec, String, Println)
│   ├── libcapsule/        # 📂 System specialized helper library
│   └── targets/           # 📜 JSON target specifications for Rustc
├── userspace/             # 📂 User-Space Sandboxed Ecosystem
│   ├── services/          # 🛡️ Sandboxed servers (init, devmgr, vfs, loader)
│   └── programs/          # 🐚 Shell, CLI applications, and tools
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
When launched using `xtask code run --arch aarch64`, the bootloader aligns, validates, and hands control over to the HNX Microkernel, which securely initiates the sandboxed EL0 environment.  The following is a real capture from `build/dist/capsuleos-pangu-0.5.9-develop-aarch64-20260711.img`:

```text
INFO  | BOOT           | Booting v0.5.9-develop...
INFO  | BOOT           | DTB found at fallback addr 0x42000000.
INFO  | BOOT           | Valid OHLINK Image Found!
INFO  | BOOT           | => Version : 1.1
INFO  | BOOT           | => Entry   : 0x0000000040080000
INFO  | BOOT           | => Segments: 1
INFO  | BOOT           | => Size    : 0x003d296c bytes
INFO  | BOOT           | Extracting payload to entry point...
INFO  | BOOT           | Jumping to HNX Kernel...
INFO  | MM             | Physical page allocator initialized.
INFO  | FDT            | Discovered hardware: UART=0x9000000, RAM=512 MiB @ 0x40000000
INFO  | MMU            | 4-level page tables ACTIVE
INFO  | IRQ            | GIC + generic timer enabled
INFO  | BOOT           | OK
INFO  | SMOKE          | VMO/VMAR smoke test: MATCH (via MMU translation)
INFO  | ROOTFS         | Found entry: 'system/bin/loader' (262 KB) -> slice 0x40233988
INFO  | LAUNCHER       | Successfully launched program 'loader' at EL0 (pid=1)
INFO  | LOADER         | Loader service launched successfully at EL0!
INFO  | SCHED          | Starting preemptive multitasking...
INFO  | SPAWN          | devmgr spawned at EL0 (pid=2)
ERROR | EL0-FAULT      | EC=0x24 thread=#1 -- KILLED thread to prevent looping exception
INFO  | SYSCALL        | Process exited with code 0
ERROR | SCHED          | No runnable threads left! Halting CPU safely...
```

---

## 📊 Project Status & Roadmap

CapsuleOS is under **active pre-1.0 development** (currently `v0.5.9-develop` on the `develop` branch).  The kernel builds and boots cleanly on both target architectures and the basic sandbox model is end-to-end functional, but several areas are explicitly **not** production-ready.  See `TODO.md` for the full list.

**Working today (verified end-to-end via `xtask code run`):**
* AArch64 (`aarch64-unknown-none`) and RISC-V 64 (`riscv64imac-unknown-none-elf`) kernels both build zero-warning.
* 4-level MMU page tables, physical page allocator, GIC + generic-timer interrupt wiring.
* `loader` → `devmgr` EL0 spawn chain (see the boot log above).
* Capability-based `Handle` table, VMOs/VMARs, SVC dispatch, IPC channels.
* OHLINK binary loader with CRC32-IEEE checksum validation and the six AArch64 relocation types.

**Planned for the 0.5.x → 0.6.0 line:**
* RISC-V 64 HAL ownership (currently an explicit roster gap — see "Known Limitations").
* EL0 fault resilience hardening (recent: scheduler Dead-thread handling, `sys_exit` reschedule, full `serror_el0` handler).
* Init anchor respawn (kernel-side spawn of `system/bin/init` when the boot anchor pid 1 dies).
* Host-side unit test infrastructure for the no_std-safe subset of `kernel/` and `libstd/`.
* CI matrix running both architectures through `cargo xtask code build` on every push.

**Not a goal before 1.0:**
* SMP / multi-core bring-up (single-CPU only by design).
* Filesystem beyond the embedded read-only rootfs.
* Networking stack.

---

## 🧰 Development Workflow

Every developer-facing rule — branching, commit style, dual-architecture compile guard, pre-commit safety grid, the `xtask` double-star toolchain — lives in a **single source of truth**: [`AGENTS.md`](./AGENTS.md).  Read that first.  Any change that contradicts it is a process bug, not a code bug.

The repository also carries a **4-rein AI dev team** under `.harness/` (orchestrator + `microkernel-architect`, `aarch64-expert`, `rust-kernel-dev`, `asm-debugger`).  When delegating work to an AI assistant, point it at `.harness/agent.md` so the routing decisions are made by the same single source of truth.

To bring up a working QEMU session from a fresh clone:

```bash
./install_xtask
xtask code build --arch aarch64
xtask code run    --arch aarch64
```

For the RISC-V target (kernel-only, no userspace QA yet — see Known Limitations):

```bash
xtask code build --arch riscv64
```

---

## ⚠️ Known Limitations

These are **explicit gaps** in the current `develop` branch, not latent bugs:

* **RISC-V 64 HAL has no dedicated owner.**  The four AI reins document this in their `Don't own` sections; AArch64 contract changes require leaving a `riscv64 impact` comment.  S-Mode CSR layout, `stvec`, `satp`, PMP and SBI bring-up are not actively maintained.
* **No init anchor respawn.**  When the EL0 process holding pid 1 (the `loader` service) faults and is killed, the kernel does not currently respawn `system/bin/init` automatically.  The system gracefully halts via `SCHED No runnable threads left` instead.  Fix is queued for 0.6.0.
* **SError handler coverage is unverified.**  The `serror_el0` path is fully implemented (save TrapFrame → dispatch → kill-thread → eret) but no test has been observed to actually trigger an SError from EL0 in 30 s of normal QEMU boot.  Production users will need a fault-injection harness.
* **Single-CPU only.**  The scheduler, MMU bring-up, and IPC paths all assume one CPU is online.  No SMP barriers, no per-CPU data.
* **No host test runner.**  `cargo test` cannot run inside the `aarch64-unknown-none` and `riscv64imac-unknown-none-elf` targets.  The host-testable subset has not yet been carved out.

---

## 📜 License

CapsuleOS is released under the **Apache License, Version 2.0** (January 2004).  You can find the full text in [`LICENSE`](./LICENSE) and the project-level attribution in [`NOTICE`](./NOTICE).  A human-readable summary — including the patent grant and the patent-retaliation termination clause — is available at <https://www.apache.org/licenses/LICENSE-2.0>.

In short: commercial use, modification, redistribution, and derivative works are permitted, provided that the `LICENSE` and `NOTICE` files travel with the binary or source distribution, any modifications are marked, and the contributors' patent grants are respected (i.e. do not file patent litigation alleging the Work infringes — §3 terminates the patent grant in that case).

The three vendored subtree projects (`bootloader/capsule-bootloader`, `kernel/hnx-core`, `tools/ohlink-cc`) carry their own upstream license texts in their respective subdirectories and remain under those original terms; see `NOTICE` for the consolidated attribution.

Trademarks ("CapsuleOS", "Pangu", "HNX-Project") are reserved by the maintainer.  Apache-2.0 §6 does not grant permission to use these in product names or marketing without prior written consent.  Contact <tinchychin97@gmail.com> for trademark enquiries.

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
