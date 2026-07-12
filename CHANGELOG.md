# Changelog

All notable changes to CapsuleOS are recorded here.  The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project aims to honour [Semantic Versioning](https://semver.org/) once
it leaves pre-1.0 development.

> **Pre-1.0 note:**  Every `0.MINOR.PATCH` release of CapsuleOS may
> contain breaking ABI changes (new `SYSCALL_*` numbers, OHLINK header
> revisions, `TrapFrame` layout changes, vector-table reshuffles).
> Pin to a specific commit hash if you depend on a frozen user-space
> ABI.  Stable ABI is the goal of the `0.9.x` line, not a guarantee
> before then.

---

## [Unreleased]

### Added
- **Kernel completeness audit** ([`AUDIT.md`](./AUDIT.md)) — full
  static scan of `kernel/src/**`, `kernel/hal`, `kernel/shared`.
  8 high-severity findings (RISC-V `csrw satp` regression,
  RISC-V `translate_user_va` stub, RISC-V EL0 launch missing
  TTBR0 init, `sys_read` returns `b"VFS_READ_OK"` literal,
  scheduler lock + IRQ-on ordering, thread-park `0x1usize` elr
  in guard page, multi-process entry-VA collision, VFS
  skeleton), ~22 medium, ~30+ low.  See `TODO.md` §"Kernel
  Completeness Audit" for the concrete deltas.

### Changed
- Deleted dead `kernel/src/kcore/alloc.rs` (contained a
  `#[global_allocator]` whose `alloc()` returned `nullptr` — a
  latent landmine for any future `extern crate alloc`).
- Deleted dead `kernel/src/kcore/debug.rs` (one-line no-op
  `panic_print` that no caller used).

### Planned for 0.5.10+
- Host-side unit tests for the no_std-safe subset of `kernel/` and
  `hnxstd/` (currently no `#[cfg(test)]` paths can actually run
  inside the `aarch64-unknown-none` or `riscv64imac-unknown-none-elf`
  targets; the host-testable subset has not been carved out).
- Init anchor respawn follow-ups:
  - SError fault-injection harness — the `aarch64_serror_el0_handler`
    path is now wired up, but no test has been observed to actually
    trigger an SError from EL0 in 30 s of normal QEMU boot.
  - `sys_execve` argv copy path is correct but currently
    unexercised by the default boot chain (init uses
    `hnxlibc::exec` (SYSCALL_EXEC, no argv), not
    `hnxlibc::execve` (SYSCALL_EXECVE, with argv)).  Exercising
    it requires changing `init/src/main.rs` to call
    `hnxlibc::execve("osh", &["osh"])` (or similar) and
    confirming `argc=1, argv[0]="osh"` reaches the new process
    via the `user entry trampoline` in `hnxlibc/src/entry/`.

### Landed since 0.5.9 (on `develop`, pending release as 0.5.10)
- **Init anchor respawn** (`555333b feat(init-anchor)`): new module
  `kernel/src/task/init_respawn.rs`.  `sys_exit`,
  `aarch64_sync_el0_handler` non-SVC branch, and
  `aarch64_serror_el0_handler` each consult
  `respawn_init_if_anchor(pid)` before marking the caller `Dead`.
  When the dying caller is the boot anchor (pid 1) and the respawn
  budget has not been spent, the kernel launches
  `system/bin/init` once.  Verified end-to-end: after the loader
  triggers an EL0-FAULT, the kernel prints
  `RESPAWN boot anchor (pid=1) died; spawned system/bin/init`,
  launches the replacement, and `init` reaches
  `init: starting…` and `init: handing off to osh` before
  entering the user-space `sys_execve` path.
- **Open-source license: Apache-2.0** — `LICENSE` now contains the
  full Apache-2.0 text (verbatim from
  <https://www.apache.org/licenses/LICENSE-2.0.txt>).  A new
  `NOTICE` file carries the project-level copyright and the
  subtree-attributed third-party projects
  (`bootloader/capsule-bootloader`, `kernel/hnx-core`,
  `tools/ohlink-cc/`).  Apache-2.0 §4(d) requires the NOTICE file
  to be redistributed alongside any binaries or modified sources,
  so the file is part of the standard source distribution.
  Trademark notice (CapsuleOS, Pangu, HNX-Project) is reserved
  per Apache-2.0 §6 — see `NOTICE` and the "License" section of
  `README.md`.

---

## [0.5.9] — 2026-07-11

### Added
- Full `serror_el0` handler in `kernel/src/arch/aarch64/boot_asm.S`
  (replaces the previous 4-line diagnostic-halt placeholder).  When
  an asynchronous SError is taken from EL0, the kernel now saves the
  full 208-byte `TrapFrame`, dispatches to
  `aarch64_serror_el0_handler` (new in `trap.rs`), and follows the
  same kill-thread + reschedule flow used by the sync EL0 fault path.
- `aarch64_serror_el0_handler` decodes the `AET` field
  (ESR_EL1.ISS[12:10]) so "uncategorised" vs. "uncontainable"
  SError sources are distinguishable in the log.
- 4-rein AI dev team under `.harness/` (orchestrator plus
  `microkernel-architect`, `aarch64-expert`, `rust-kernel-dev`,
  `asm-debugger`).
- `AGENTS.md` is now the single source of truth for developer-facing
  rules (branching, commit style, dual-architecture compile guard,
  pre-commit safety grid, the `xtask` double-star toolchain).

### Changed
- `pop_next_from_all_queues` in the scheduler now skips entries
  whose state is `Dead`, and falls back to a linear scan of
  `self.threads` for any non-Dead slot when the ready queues are
  exhausted.  Without this, a user thread killed by an EL0 fault
  would be re-selected as the next runnable thread, and the
  `prev == next` short-circuit would loop forever in
  `SCHED-SAME`.
- The `prev == next` short-circuit in `schedule()` is now gated on
  `prev_state != ThreadState::Dead`, so a freshly-killed thread
  cannot "win" the short-circuit and force the trap epilogue to
  `eret` back into a dead context.
- `sys_exit` now marks the calling thread `Dead` and calls
  `SCHEDULER.schedule()` instead of `loop {}`-ing in place.  A
  user program that voluntarily exits (e.g. `devmgr`) previously
  trapped the kernel in `SCHED-SAME` because the calling thread
  stayed in the `Running` state from the scheduler's point of view.
- `README.md` and `TODO.md` updated for 0.5.9 (version badge, boot
  output, Status/Roadmap/Development Workflow/Known Limitations
  sections, new Phase 5.5 EL0 trap resilience log).

### Fixed
- **EL0-FAULT `SCHED-SAME` dead loop** when the faulting thread was
  still resident in a priority queue.  Symptom: kernel hangs after
  the first user-space fault instead of switching to the next
  runnable thread.
- **Process-exit `SCHED-SAME` dead loop** when the last live user
  process exits via `sys_exit`.  Symptom: kernel hangs in
  `SCHED-SAME prev == next == N (skip switch_to hijack)` instead of
  falling through to the "no runnable threads" halt path.

### Not yet verified
- `serror_el0` has not been observed to fire during a 30 s QEMU
  boot.  The handler is in the binary and ready, but its end-to-end
  behaviour (log + kill thread + eret into next thread) has not
  been confirmed against a real SError.  A fault-injection harness
  is needed before this path can be marked "tested".

---

## [0.5.0] — Pangu subtree monorepo + OHLINK toolchain (5.0 / 5.1 / 5.2 / 5.3)

### Added
- Subtree monorepo: legacy `kernel` / `bootloader` / `ohlink-cc`
  submodules squashed into the main repository with dedicated
  subtree remotes (`kernel-up`, `bootloader-up`, `ohlink-cc-up`).
- `ohlink-format` crate: a `#![no_std]` strongly-typed parser for
  the OHLINK binary container (`OHLK_Header` + `OHLK_Entry`).
- Microkernel OHLINK loader (rewritten `sys_exec`): CRC32-IEEE
  validation, page-aligned segment injection, strict `Text / Data /
  Rodata / Bss` boundaries.
- `xtask` double-star toolchain: split into `xtask code` (build,
  run, check-env) and `xtask repo` (commit, push, setup-fork,
  pull, release).  Pre-commit safety grid runs `rustfmt`, a
  workspace-wide `cargo check` (excluding bootloader), and both
  AArch64 and RISC-V 64 cross-compile targets.

### Changed
- Local git identity locked to `TinchyChin <tinchychin97@gmail.com>`
  on the capsule-os repository.
- Bootloader accepts the new `OHLK` magic number for first-stage
  loading.
- All `userspace/` programs now build via the
  `-Zcodegen-backend` Rust nightly feature wired through
  `ohlink-cc`.

---

## [0.4.0] — IPC + capability ownership transfer

### Added
- `Channel`: zero-allocation synchronous rendezvous IPC with
  `send_waiters` / `recv_waiters` static FIFO queues and direct
  handoff single-copy semantics.
- `Port`: 32-byte async completion port using page-allocated ring
  buffers and per-TCB `port_packet_slot`, eliminating
  kernel-heap-exhaustion attack surface.
- Per-process `HandleTable` isolation (each process owns its own
  capability table; the historical shared-global table is gone).
- `HandleTable::remove_with_rights` / `add` for safe cross-process
  capability transfer during `channel_write` rendezvous.
- Multi-process isolation VMO transfer smoke test
  (`sender creates VMO → write → channel → receiver revokes
  sender's view → re-maps → reads back`).

---

## [0.3.0] — Preemptive scheduling + interrupts + capabilities

### Added
- AArch64 and RISC-V 64 trap-context save/restore assembly
  (`TrapFrame` covering 31 general-purpose registers + privileged
  CSRs).
- GIC (AArch64) and PLIC (RISC-V 64) interrupt controllers wired
  to a hardware-timer tick source.
- Vector table (`vector_table` in `boot_asm.S`) with safe
  dispatch into the Rust scheduler.
- `Thread` (TCB) and `Process` (PCB) types: TCB holds the kernel
  stack pointer and the `TrapFrame` address; PCB owns the root
  `VMAR` and the private `HandleTable`.
- Zircon / seL4-style capability model: every kernel object
  (VMO, VMAR, Channel, Thread) is referenced by a `u32` `Handle`
  with explicit `READ / WRITE / EXECUTE / MAP` rights.
- Adaptive multi-level feedback queue (MLFQ) scheduler with
  round-robin time slices and priority decay.
- `switch_to` assembly: saves the AAPCS64 callee-saved register
  window (`x19`-`x29` / `s0`-`s11`) plus `SP` and `LR`.

---

## [0.2.0] — FDT + MMU + VMO/VMAR

### Added
- FDT (Flattened Device Tree) parser: dynamically locates RAM
  range and PL011 / NS16550 UART MMIO base (replaces hardcoded
  constants; enables multi-board support).
- Physical page allocator: implicit free-list, 0-byte metadata,
  O(1) `allocate_page` / `free_page`.
- 4-level MMU page tables on AArch64, SV39 on RISC-V 64:
  identity map, kernel high-half map at `KERNEL_OFFSET =
  0xFFFF_8000_0000_0000`, Device block for UART.
- `MMU` activated on both targets (`SCTLR_EL1` / `TCR_EL1` /
  `MAIR_EL1` / `TTBR*_EL1` writes followed by `TLBI VMALLE1 +
  DSB SY + ISB`).
- `VMO` (Virtual Memory Object): 4 KiB granularity, lazy
  allocation, page-fault-style `commit_page`, `read` returns 0
  for uncommitted pages.
- `VMAR` (Virtual Memory Address Range): tree structure, root
  + sub-regions, `map` / `unmap` / `protect` API, 1 GiB L1 and
  2 MiB L2 "block" entries are shattered on demand.
- Cross-architecture `MapFlags` (`kernel_rw / ro / rx`, `user_rw /
  ro`, `device_rw`) and a `map_page / unmap_page / pa_to_kernel_va`
  contract in `arch::mmu`.
- `vmo_vmar_smoke_test()` (kernel boot-time smoke): AArch64
  verifies the data round-trip via MMU translation; RISC-V
  verifies via direct `pa_to_kernel_va` (no MMU translation path
  exercised on RISC-V SV39 bring-up).

---

## [0.1.0] — Pangu first boot

### Added
- `ohc-tool` and OHC binary format packing (CRC32 + payload
  trailer).
- `capsule-bootloader` first-stage: multicore park (CPU 0 boots,
  others enter `wfi`), FDT physical address propagation via `x0`.
- `boot_asm.S` linker-symbol-driven stack and BSS allocation
  (replaces hardcoded physical addresses).
- `kernel_main` accepts the FDT pointer and stores it in
  `DTB_POINTER`.
- `vector_table` minimal stubs for `sync` / `irq` traps.
- PL011 UART early register-level bring-up via
  `arch::console_putchar`.
- First successful QEMU boot: `CapsuleOS v0.1.0` and `OK`
  printed to the UART.

[Unreleased]: https://gitcode.com/hnx-project/capsule-os/compare/0.5.9...HEAD
[0.5.9]: https://gitcode.com/hnx-project/capsule-os/compare/0.5.0...0.5.9
[0.5.0]: https://gitcode.com/hnx-project/capsule-os/compare/0.4.0...0.5.0
[0.4.0]: https://gitcode.com/hnx-project/capsule-os/compare/0.3.0...0.5.0
[0.3.0]: https://gitcode.com/hnx-project/capsule-os/compare/0.2.0...0.3.0
[0.2.0]: https://gitcode.com/hnx-project/capsule-os/compare/0.1.0...0.2.0
[0.1.0]: https://gitcode.com/hnx-project/capsule-os/releases/tag/0.1.0

---

## [1.0.0-beta] - 2026-07-12 - "Pangu" (pre-release)

**This is a `-beta` pre-release.**  Workspace version
`1.0.0-beta`; semver pre-release means `1.0.0-beta > 0.9.x` and
`1.0.0-beta < 1.0.0`.  Image name suffix:
`capsuleos-pangu-1.0.0-beta-aarch64-<date>.img`.  Targets a
milestone where `cat /etc/hostname | grep .` runs end-to-end
from `init` once A2 (`KERNEL_HEALTH.md`) closes; the current
boot chain reaches `devmgr`'s banner-print but hits a
`SError=0x0f` on the very first stack access past
`sys_spawn("devmgr")` (already documented in `RELEASE.md`
"Known issues").

[1.0.0-beta]: https://gitcode.com/hnx-project/capsule-os/releases/tag/v1.0.0-beta

(For purposes of the Keep a Changelog 1.1.0 "Added / Changed /
Fixed / Removed" entries, this 1.0.0-beta section is the same
content as the previously-drafted 1.0.0 entry that was
re-tagged pre-release after the workspace-version bump in
`Cargo.toml` rewrote `version = "1.0.0"` to `"1.0.0-beta"`.)

### Highlights
This is the first **stable-API** CapsuleOS release.  The
Pangu codename carries the EL0 POSIX surface from 0.6 to a
kernel ABI that every program in `userspace/programs/` and
`userspace/services/` can compile against.

### Added (kernel)
- `SYSCALL_WAIT4 (88)` - `wait4(pid, status_ptr, options)`.
  Parents can now harvest child exit codes.  Per-process
  `exit_status + parent_pid` tracked via `Process::exit_status`
  / `Process::parent_pid`.  See commit `e26f5b8` (B4).
- `SYSCALL_SIGACTION (92), SYSCALL_RAISE (93), SYSCALL_KILL (94),`
  `SYSCALL_PAUSE (95)` — POSIX signal/raise/kill/sigaction/pause
  set.  Disposition is `SIG_DFL (0)` / `SIG_IGN (1)` only for
  1.0 (custom user-mode handlers land in 1.1).  See commit
  `9295fc7` (B5).
- `SYSCALL_PIPE (96), SYSCALL_DUP2 (97)` — in-kernel pipe (4 KiB
  ring buffer) + per-process `fd_table` of size 16.  Pipe halves
  indexed by user-fd, kernel-owned 0/1/2 stay on K-D2 UART.
  See commit `8a32b8b` (B6).
- aarch64 **AP-bits fix** on L3 PTE writes for user-writable
  pages: pre-1.0 set bit 6 only, producing `AP[2:1]=01`
  ("EL0 forbidden") on the loader's stack; 1.0 sets both 6 + 7
  producing `AP[2:1]=11` ("EL0 RW").  See commit `f4f601a`
  (B1.5 partial close of KERNEL_HEALTH.md A2).

### Added (userspace / runtime)
- `hnxlibc::pipe`, `dup2`, `wait4`, `getppid`, `sigaction`,
  `raise`, `kill`, `pause`, `pipe_pair` C-ABI wrappers.
- EL0 `panic_handler` (B10): pre-1.0 was a silent `loop {}`.
  1.0 prints `EL0 PANIC: <msg> @ <file>:<line>` to fd 2 before
  halting the processor.  Source location comes from the
  compiler's `#[track_caller]` metadata.
- `hnxstd` (B9) now ships `vec::Vec<T>`, `string::String`,
  `fmt::format!`, `write!` macros for EL0 ELF programs.

### Added (build)
- xtask `stage_etc_files()` recursively copies `kernel/files/
  etc/` into the staging tree before pack_rootfs.  1.0
  ships `capsule-os-pangu-1.0.0-beta` as `/etc/hostname` and a
  LSB-style `/etc/os-release`.  See commit `f407929` (B8).

### Added (shell)
- osh `parser.rs` now returns a `Pipeline` (up to 4 stages
  joined by `|`).  `shell.rs` dispatches single-stage via
  the existing fast path and multi-stage via `spawn() +
  yield_cpu() + wait()`.  See commit `1dd1261` (B7).

### Stable API surface

- SYSCALL numbering from `kernel/shared/src/syscall_nums.rs`
  is the canonical contract.  1.0.0-beta and 1.0 keep the 49
  assigned numbers; the 12 reserved numbers (CHANNEL_CALL,
  PORT_*, EVENT_*, TIMER_*, FUTEX_* along with the
  unconnected PMEM_*) are now numbered and documented.
- `Process` / `Thread` / `TrapFrame` layouts are frozen.
- C ABI exposed by `hnxlibc` is frozen.

### Known issues

- **A2 EL0-FAULT T10** (`KERNEL_HEALTH.md`) is partially
  closed (B1.1–B1.5 + B1.6 SError 0x0f on the second pass);
  the boot chain reaches the devmgr-banner point but stops
  short of fileagent registration.  `1.0.1` / next
  `1.0.0-beta.N+1` will close the page-walk half by demoting
  the always-on B1.4 hex dump back to `cfg(debug_assertions)`
  after the root cause is finalised.
- RISC-V (K-D3) was deliberately parked per the 1.0 scope
  discussion.  RISC-V boot is gated by the 5 mnemonic
  errors visible in `cargo xtask code build --arch riscv64`.

---

## [1.0.0] - 2026-07-12 - "Pangu"

### Highlights
This is the first **stable** release of CapsuleOS.  The Pangu
codename carries the EL0 POSIX surface from 0.6 to a kernel
ABI that every program in `userspace/programs/` and
`userspace/services/` can compile against.

### Added (kernel)
- `SYSCALL_WAIT4 (88)` - `wait4(pid, status_ptr, options)`.
  Parents can now harvest child exit codes.  Per-process
  `exit_status + parent_pid` tracked via `Process::exit_status`
  / `Process::parent_pid`.  See commit `e26f5b8` (B4).
- `SYSCALL_SIGACTION (92), SYSCALL_RAISE (93), SYSCALL_KILL (94),`
  `SYSCALL_PAUSE (95)` — POSIX signal/raise/kill/sigaction/pause
  set.  Disposition is `SIG_DFL (0)` / `SIG_IGN (1)` only for
  1.0 (custom user-mode handlers land in 1.1).  See commit
  `9295fc7` (B5).
- `SYSCALL_PIPE (96), SYSCALL_DUP2 (97)` — in-kernel pipe (4 KiB
  ring buffer) + per-process `fd_table` of size 16.  Pipe halves
  indexed by user-fd, kernel-owned 0/1/2 stay on K-D2 UART.
  See commit `8a32b8b` (B6).
- aarch64 **AP-bits fix** on L3 PTE writes for user-writable
  pages: pre-1.0 set bit 6 only, producing `AP[2:1]=01`
  ("EL0 forbidden") on the loader's stack; 1.0 sets both 6 + 7
  producing `AP[2:1]=11` ("EL0 RW").  See commit `f4f601a`
  (B1.5 partial close of KERNEL_HEALTH.md A2).

### Added (userspace / runtime)
- `hnxlibc::pipe`, `dup2`, `wait4`, `getppid`, `sigaction`,
  `raise`, `kill`, `pause`, `pipe_pair` C-ABI wrappers.
- EL0 `panic_handler` (B10): pre-1.0 was a silent `loop {}`.
  1.0 prints `EL0 PANIC: <msg> @ <file>:<line>` to fd 2 before
  halting the processor.  Source location comes from the
  compiler's `#[track_caller]` metadata.
- `hnxstd` (B9) now ships `vec::Vec<T>`, `string::String`,
  `fmt::format!`, `write!` macros for EL0 ELF programs.

### Added (build)
- xtask `stage_etc_files()` recursively copies `kernel/files/
  etc/` into the staging tree before pack_rootfs.  1.0
  ships `capsule-os-pangu-1.0` as `/etc/hostname` and a
  LSB-style `/etc/os-release`.  See commit `f407929` (B8).

### Added (shell)
- osh `parser.rs` now returns a `Pipeline` (up to 4 stages
  joined by `|`).  `shell.rs` dispatches single-stage via
  the existing fast path and multi-stage via `spawn() +
  yield_cpu() + wait()`.  See commit `1dd1261` (B7).

### Stable ABI

- SYSCALL numbering from `kernel/shared/src/syscall_nums.rs`
  is the canonical contract.  1.0.* keeps the 49 assigned
  numbers; the 12 reserved numbers (CHANNEL_CALL,
  PORT_*, EVENT_*, TIMER_*, FUTEX_* along with the
  unconnected PMEM_*) are now numbered and documented.
- `Process` / `Thread` / `TrapFrame` layouts are frozen.

### Known issues

- **A2 EL0-FAULT T10** (`KERNEL_HEALTH.md`) is partially
  closed (B1.1–B1.5 + B1.6 SError 0x0f on the second pass);
  the boot chain reaches the devmgr-banner point but stops
  short of fileagent registration.  1.0.1 will close the
  page-walk half by demoting the always-on B1.4 hex dump
  back to `cfg(debug_assertions)` after the root cause
  is finalised.
- RISC-V (K-D3) was deliberately parked per the 1.0 scope
  discussion.  RISC-V boot is gated by the 5 mnemonic
  errors visible in `cargo xtask code build --arch riscv64`.

[1.0.0]: https://gitcode.com/hnx-project/capsule-os/releases/tag/v1.0.0
