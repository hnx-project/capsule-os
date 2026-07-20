# Changelog

All notable changes to CapsuleOS are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project honors [Semantic Versioning](https://semver.org/).

---

## [1.0.0-beta] - 2026-07-20

### Added
- Standard POSIX C-ABI filesystem interfaces in `libc` (`open`, `close`, `read`, `write`, `stat`, `readdir`, `mkdir`, `rmdir`, `unlink`).
- The `testall` automated verification suite program verifying 12 system/filesystem APIs.
- Dedicated `DEVELOPMENT.md` defining core engineering and testing standards.

### Fixed
- Thread wake-up scheduler starvation during IPC rendezvous.
- Mutex deadlock in `HandleTable` by releasing locks before blocking/yielding.
- Use-After-Free memory safety vulnerability in multi-process channels via Box pointer stability.

### Changed
- Reconstructed `fileagent` as a zero-polling, single-threaded blocking event loop.
- Streamlined `README.md` into a modular, high-cohesion index document.
- Focused codebase strictly on the AArch64 target.
- Removed redundant microkernel and user-space debug logs to ensure silent execution.

---

## [0.5.9] - 2026-07-11

### Added
- Full asynchronous `serror_el0` fault handler in AArch64 trap logic.
- Structured AI Agent orchestration guidelines in `AGENTS.md`.

### Changed
- Improved scheduler linear thread scans to handle freshly-killed dead threads safely without `SCHED-SAME` deadloops.
- Voluntarily exiting processes now mark thread contexts as `Dead` and invoke scheduling.

### Fixed
- Faulting EL0 thread residence deadloops in priority queues.
- Single process exit infinite schedules.

---

## [0.5.0] - 2026-05-18

### Added
- Subtree Monorepo: unified `kernel`, `bootloader`, and `ohlink-cc` codebases under standard subtree tracking.
- `ohlink-format` crate: `#![no_std]` strongly-typed parser for the custom OHLINK binary container.
- OHLINK static segment dynamic loader executing strict MMU RX/RW/NX boundary protections.
- Companion toolchain launcher `xtask` supporting fast `code` compiling and emulation.

---

## [0.4.0] - 2026-04-12

### Added
- `Channel`: Zero-allocation synchronous rendezvous IPC with direct stack-to-stack copy.
- `Port`: Async completion ports utilizing page-allocated ring buffers.
- Unified `HandleTable` isolation giving each process local capability scoping.

---

## [0.3.0] - 2026-03-05

### Added
- Preemptive MLFQ (Multi-Level Feedback Queue) scheduler with round-robin time slicing and priority decay.
- Interrupt vector tables and GIC hardware-timer tick routing on AArch64.
- Capability delegation model: `READ`, `WRITE`, `EXECUTE`, `MAP` rights validations on syscall boundaries.

---

## [0.2.0] - 2026-02-14

### Added
- Device Tree (FDT) parsing for dynamic physical RAM discovery and early PL011 UART mapping.
- O(1) physical page allocator with implicit zero-metadata free lists.
- 4-level MMU virtual memory mapping support on AArch64.
- `VMO` (Virtual Memory Object) lazy-commit pages and `VMAR` (Virtual Memory Address Region) dynamic address space trees.

---

## [0.1.0] - 2026-01-10

### Added
- Initial bare-metal `capsule-bootloader` executing multicore park and argument propagation.
- Register-level PL011 UART early serial outputs.
- First successful boot print to serial console in QEMU.

---
*Designed and engineered with passion by **TinchyChin** and the **HNX-Project** community.*
