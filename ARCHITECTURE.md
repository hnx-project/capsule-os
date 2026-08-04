# 🌌 CapsuleOS Technical Architecture Specification

This document provides a comprehensive technical breakdown of **CapsuleOS (Current Version: Pangu 1.0.0.beta)**. It details the layered runtime execution, microkernel invariants, capability security, and process/thread models on **AArch64**.

---

## 1. ⚙️ Four-Layer Standard Runtime

CapsuleOS enforces a strict, hierarchical layered topology. Layers can only depend on equal or lower levels; dependency leaks upward are strictly prohibited.

```text
┌──────────────────────────────────────────────────────────────────┐
│  L4  User Applications (testall, osh, ls, cat, mkdir, rm, …)     │
│      EL0 (U-mode) | Sandboxed. Communicates solely via POSIX.    │
│      Builds against custom libstd/libc and standard C C-ABI.     │
├──────────────────────────────────────────────────────────────────┤
│  L3  System Services (devmgr, fileagent)                         │
│      EL0 (U-mode) | Sandboxed. OS runtime services. Operates via │
│      non-polling, blocking message loops to serialize VFS.       │
├──────────────────────────────────────────────────────────────────┤
│  L2  Userspace Support Libraries (libc, libstd, libcapsule)      │
│      EL0 Helper libraries. Translates raw syscall entries to safe│
│      Rust namespaces and standard POSIX-compliant headers.       │
├──────────────────────────────────────────────────────────────────┤
│  L1  HNX Microkernel (hnx-core)                                  │
│      EL1 (AArch64). The only privileged layer. Direct MMU       │
│      ownership, preemptive scheduler, GIC, and Handle tables.     │
├──────────────────────────────────────────────────────────────────┤
│  L0  capsule-bootloader                                          │
│      AArch64 EL3. Performs initial low-level RAM bringup,        │
│      unpacks the OHC binary, validates checksums, jumps to EL1.  │
└──────────────────────────────────────────────────────────────────┘
```

---

## 2. 🗂️ Unified Monorepo Layout

```text
capsule-os/                         # Root Folder
├── README.md                       # Public Quickstart Index
├── DEVELOPMENT.md                  # The 10 Engineering & Testing Standards
├── ARCHITECTURE.md                 # System Architecture Specifications
├── CONTRIBUTING.md                 # Development & Commit Guide
├── AGENTS.md                       # AI Agent Guidelines & Invariants
├── CHANGELOG.md                    # Keep a Changelog Release History
│
├── install_xtask                   # Host launcher setup script
├── bootloader/                     # Subtree: Bare-metal AArch64 Bootloader (L0)
├── kernel/                         # Subtree: HNX Core Microkernel (L1)
│   ├── linker/                     # aarch64 Linker Scripts
│   └── src/                        # Rust Core Source Files
│       ├── arch/aarch64/           # CPU instructions, MMU, vectors, traps (HAL)
│       │   └── drivers/            # SoC-specific drivers (PL011, GIC, Timer)
│       ├── core/                   # Scheduler, Process, Thread, VM, and Locks (Microkernel)
│       ├── posix/                  # High-performance in-kernel POSIX fast-paths (pipe, shm)
│       ├── pills/                  # Kernel-level Pill loader module
│       └── lib.rs                  # Microkernel entry and initialization
│
├── pillsmod/                       # 💊 Plug-in high-privilege peripheral driver bundles (EL1/EL0.5)
│   ├── graphicd/                   # VirtIO-GPU display driver (.pill)
│   ├── inputd/                     # VirtIO-Mouse input driver (.pill)
│   └── touchd/                     # VirtIO-Touch multi-touch driver (.pill)
│
├── libraries/                      # Sandboxed Runtime Support Libraries (L2)
│   ├── libc/                       # Standard POSIX translation layer (C-ABI)
│   ├── libstd/                     # Self-built Rust standard library
│   ├── libcapsule/                 # Safe system call wrapper with Error statuses
│   ├── libtty/                     # Terminal line discipline / TTY library
│   └── targets/                    # aarch64-unknown-capsule target JSON
│
├── services/                       # 📂 Sandboxed Daemon Services (L3)
│   ├── servicesd/                  # 1️⃣ Init & Dynamic DAG service manager
│   ├── fileagent/                  # VFS fileagent (Ext2/FAT/Ramfs)
│   ├── blkdev/                     # Block storage device service
│   ├── devmgr/                     # Hardware probing & FDT service
│   ├── loader/                     # User-space OHLINK binary loader
│   └── netd/                       # TCP/IP network protocol stack
│
└── apps/                           # 📂 User Applications & Shells (L4)
    ├── shell/                      # Interactive terminal shells (osh, bash)
    └── utilities/                  # Standard command-line POSIX tools
```

---

## 3. 🛡️ Capability Security Model & Handle Table

CapsuleOS operates on a zero-trust model. Sandbox processes cannot access physical memory or hardware directly, nor can they index kernel resources using virtual memory addresses.

### 3.1 `HandleTable` & `HandleValue`
*   Every process owns a private, sparse `HandleTable`.
*   A `HandleValue` is a process-local 32-bit integer indexing a specific slot in that table.
*   Each handle encapsulates a strong-typed `KernelObject` reference paired with `Rights` (bitmask permissions like `READ`, `WRITE`, `EXECUTE`, `DUPLICATE`, `TRANSFER`).

### 3.2 Invariant: Decoupled Locking for Blocking Operations
To guarantee high performance and absolute safety against deadlocks:
*   A thread must never enter a waiting or block state inside the scheduler while holding a lock on its parent `HandleTable`.
*   *Implementation*: Retrieve raw references to required resources (such as `Channel` structs), drop the handle table lock safely, and then perform blocking synchronous read/write or yield operations.

### 3.3 Invariant: Stable Pointer Protection (UAF Prevention)
When capability handles are transferred from a sender to a receiver across process boundaries:
*   The raw memory of the underlying `Channel` or `Port` must never move or reallocate.
*   *Implementation*: All underlying channel structures are wrapped inside heap-allocated `Box` pointers (e.g., `KernelObject::Channel(Box<Channel>)`) upon registration. This ensures that their heap memory addresses remain statically fixed in the heap throughout their lifecycle, ensuring peer references are always stable and never become dangling pointers.

---

## 4. 🚀 Zero-Allocation IPC (Synchronous Rendezvous)

IPC Channels in CapsuleOS do not allocate any temporary heap blocks or intermediate message buffers during message transfers.

### 4.1 Rendezvous Principle
*   **Direct Stack-to-Stack Handoff**: If Thread A writes to a channel while Thread B is waiting to read, the microkernel performs a direct, single-pass memory copy (`memcpy`) from Thread A’s userspace buffer directly into Thread B’s userspace buffer.
*   **Capability Transfer**: Any capability handles included in the message are safely uninstalled from the sender's `HandleTable` and installed in the receiver's `HandleTable` under the same system call boundary.
*   **Waking Integration**: Once the copy is completed, the waiting thread is moved to the active run queue via `SCHEDULER.wake_thread(tid)` to prevent starvation under yield loops.

---

## 5. 💾 Virtual Memory Model (VMAR & VMO)

Virtual memory is partitioned into two modular concepts:

### 5.1 Virtual Memory Objects (VMO)
*   Represent contiguous, physical-page-backed allocations.
*   Supports lazy committing: pages are allocated and mapped on demand.
*   Fully reference-counted; physical frames are automatically reclaimed when the last referring handle is closed.

### 5.2 Virtual Memory Address Regions (VMAR)
*   Represent tree-like hierarchial slices of the process's virtual address space.
*   Each VMAR maintains explicit MMU hardware permission bounds (e.g., RX for text segments, RW/NX for stack/data).
*   Fine-grained page mapping/unmapping is executed by updating the CPU's 4-level MMU translations on page allocation.

---

## 6. 🛠️ OHLINK Custom Binary Standard

To completely separate CapsuleOS from standard GNU ELF overheads and dependencies, userspace files are compiled and loaded using our custom **OHLINK** format.

### 6.1 Executable Structure
1.  **Header (48-Bytes)**: Stores magic string `0x4F484C4B` ("OHLK"), file size, header entry count, and a strict CRC32-IEEE checksum (calculated with the checksum field set to 0).
2.  **Section Table**: Descriptor table of loadable segments.
    *   `TYPE_TEXT` (`0x00010001`): Executable code, mapped RX.
    *   `TYPE_DATA` (`0x00020002`): Initialized global data, mapped RW/NX.
    *   `TYPE_RODATA` (`0x00030003`): Constants, mapped RO/NX.
    *   `TYPE_BSS` (`0x00040004`): Zero-initialized variables, allocated dynamically.

### 6.2 Relocation Mechanics
Our self-built compiler backend support 6 distinct AArch64 relocation equations at static load-time:
*   `R_AARCH64_ABS64`
*   `R_AARCH64_CALL26`
*   `R_AARCH64_ADR_PREL_PG_HI21`
*   `R_AARCH64_ADD_ABS_LO12_NC`
*   `R_AARCH64_LDST64_ABS_LO12_NC`
*   `R_AARCH64_ADR_PREL_LO21`

---

## 7. 💊 Pangu-Hybrid: Pills-ification & SoC Driver HAL Isolation

To address the latency, throughput, and initialization deadlocks traditional to pure microkernels, CapsuleOS incorporates a hybrid design separating core board bring-up from dynamic high-performance peripheral modules.

### 7.1 SoC-specific Driver HAL Isolation
All SoC-specific system-level drivers (Generic Interrupt Controller, ARM Generic Timer, and UART consoles pl011/ns16550) are isolated under the Hardware Abstraction Layer (HAL) at `kernel/src/arch/aarch64/drivers/`.
*   **Decoupled Interface**: The general microkernel core (`kernel/src/lib.rs` and core subsystems) accesses platform hardware solely through generic interfaces defined in `kernel/src/arch/mod.rs` (e.g. `early_console_init()`, `interrupts_init()`, and `get_ticks()`).
*   **Target Agnosticism**: This isolation guarantees that the core microkernel is completely board-independent, facilitating seamless porting to other architectures (like RISC-V or x86) by merely implementing their respective HAL facades.

### 7.2 High-Performance Dynamic ".pill" Bundles
Plug-in peripheral drivers (VirtIO-GPU, VirtIO-Mouse, VirtIO-Touch, and eventually high-throughput services like network protocol stacks and block device managers) are designed as **Pills (Kernel Extensions)** located in `/pillsmod/`.
*   **Pill Bundle Structure**: Each pill is packed as a directory bundle containing its custom hardware metadata manifest (`auto.toml`) and its compiled OHLINK binary (e.g. `system/lib/pills/graphicd.pill/`).
*   **Dual-Mode Capability**: These components can be compiled to run as isolated EL0 daemon processes (for security and online debugging) or dynamically loaded by the kernel-level `pill_loader.rs` in EL1 (for zero-copy high-throughput DMA and low-latency interrupt routing).
*   **BootFS Staging**: `.pill` bundles are built and staged into the root filesystem archive (`rootfs.img`) dynamically before userspace boot, preventing filesystems-not-ready initialization deadlocks.

---
*Designed and engineered with passion by the **HNX-Project** community.*
