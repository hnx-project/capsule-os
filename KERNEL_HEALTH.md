# KERNEL_HEALTH.md - CapsuleOS Kernel Health Table (POSIX-aligned)

> **Generated:** 2026-07-11
> **Scope:** `kernel/src/**`, `userspace/{hnxlibc,hnxstd,services}`,
> `tools/ohlink-cc`, plus the static-analysis findings of
> [`AUDIT.md`](./AUDIT.md).
>
> **This is the planning table, not the implementation table.**
> For implementation commits see [`CHANGELOG.md`](./CHANGELOG.md);
> for raw severity roll-ups of the source modules see
> [`AUDIT.md`](./AUDIT.md).
>
> The headline conclusion: **CapsuleOS is a working microkernel
> bring-up with ~85% functional completeness, ~10% path-verification
> completeness, and 0% POSIX syscall-surface coverage at EL0.**
> Every `open(2) / read(2) / write(2) / lseek(2) / close(2)` that
> an EL0 program can issue today returns either the literal byte
> string `VFS_READ_OK` (H4) or a freshly-allocated empty `Vnode`
> (H8).  The userland libc (`hnxlibc`) routes around this by talking
> directly to fileagent through `sys_channel_*`, so the bring-up
> works *in practice* — but the kernel syscall surface as exposed
> to EL0 is a stub, not a router.  Phase 6 v0.6.0-α is the wire-up.

## Knobs / Downscope decisions

The following are **deliberate, documented choices**, not unfinished
work — they shape what items appear in the table below and what
"target" we aim for.

- **K-D1 — fork-less POSIX.**  CapsuleOS will **not** implement
  `fork(2)` in 1.0.  We follow the modern container model
  (`posix_spawn`, `CLONE_VM`-style semantics) — every spawned
  program starts from a clean `Process::new`, with no
  address-space duplication.  This eliminates the fork-related
  M3 (`vmar::map` PTE leak) / M11 (per-process L0 init regression)
  complications in one stroke and is consistent with how Docker,
  systemd, and most modern init systems actually launch processes.
  `libc::posix_spawn` is in-scope for 0.7; raw `fork()` is not.

- **K-D2 — Character I/O stays in the kernel.**  `sys_read fd=0`
  (UART stdin) and `sys_write fd=1/2` (UART stdout/stderr) are
  **deliberately** in-kernel — early boot has no fileagent to
  forward to.  These two paths are the only stubs that stay
  syscall-handled and not forwarded.  They are the only P-bucket
  items marked `working` rather than `latent / PosixBroken`.

- **K-D3 — RISC-V parked.**  Per the AUDIT headline and
  `TODO.md` §"RISC-V 引导在 MMU 之前就挂了", the RISC-V path
  is half-finished and out of scope until a dedicated rein is
  stood up.  AArch64 is the only working target for 0.6-α / 0.7
  / 1.0.

## Status key

| Marker | Meaning |
|---|---|
| `working` | Live on the boot path; QEMU-observed; logs prove it. |
| `code-ready-not-verified` | Implementation complete; QEMU path not yet exercised or not provable from logs alone. |
| `latent` | Not triggered by the current boot path; analysis says it will break under future conditions (EL0 raw-syscall caller, SMP, IRQ-嵌套, etc). |
| `not-started` / `parked` | Explicitly deferred or parked for later phase. |

Row tags: `PosixBroken = current bug is wrong / stub POSIX
behaviour that will be seen by any EL0 program that bypasses
hnxlibc (i.e. raw `__NR_open` / hand-written aarch64 SVC #0
calls).`

---

## Bucket 1 - POSIX Libc Surface (kernel syscall layer)

| # | Item | State | Notes |
|---|------|-------|-------|
| **P1** | `sys_open` (K1/H8) | `latent / PosixBroken` | `vfs/handlers/vfs.rs:4-18` allocates a fresh `Vnode` per call with `size=0` and **never reads any rootfs content**. Returns `Ok(fd)` unconditionally. An EL0 program that bypasses hnxlibc and issues `open("/etc/passwd")` will succeed, get a fd, then `read()` from it returns `b"VFS_READ_OK"`. **0.6-α target:** convert to forwarder that resolves `svc.vfs`, sends `FileAgentCmd::Open`, parses the response, and stores `(process_id, fd) → LibcFile` in the per-process POSIX fd table (P7). |
| **P2** | `sys_read` fd≥3 (K2/H4) | `latent / PosixBroken` | `vfs/handlers/vfs.rs:31-70` returns `b"VFS_READ_OK"` literal (11 bytes) for any non-stdin fd. **0.6-α target:** forwarder: resolve fd→`{session_chan, remote_fd}` from per-process POSIX fd table, issue `FileAgentCmd::Read` over channel, **use `safe_copy_to_user`** to fill the caller buffer. fd=0 stays on the UART path (K-D2). |
| **P3** | `sys_write` fd≥3 (K5) | `latent / PosixBroken` | `handlers/mod.rs:8-52` returns `Status::NotAllowed` for any fd>2. **0.6-α target:** forwarder: resolve fd→`{session_chan, remote_fd}`, `safe_copy_from_user` into a 256-byte temp buffer, send `FileAgentCmd::Write` (the variant that ships a VMO handle for the buffer body) over channel. |
| **P4** | `sys_close` (K3/H8) | `latent / PosixBroken` | `vfs/handlers/vfs.rs:20-29` releases the **stub Vnode** — there is no real file. **0.6-α target:** forwarder: lookup fd in POSIX fd table, send `FileAgentCmd::Close`, free the slot. |
| **P5** | `sys_lseek` (K4) | `latent / PosixBroken` | `vfs/handlers/vfs.rs:72-89` writes a local offset counter against the stub Vnode. **0.6-α target:** forwarder: send `FileAgentCmd::Seek` (`whence` is an enum on the wire); or maintain `offset` kernel-locally and push on each `Read`/`Write`. Either is acceptable; the cheaper one wins. |
| **P6** | `sys_getpid` / `sys_gettid` | `not-started / PosixBroken` | `syscall/numbers.rs` defines `SYSCALL_GET_TID=2`, `SYSCALL_GET_PID=3` but `syscall_dispatch` (AUDIT M1) has no arm. Any EL0 caller gets `Status::NotAllowed`. **0.6-α target:** wire to `Scheduler::get_current_thread().process_id` / `.id`. Cheap (one match arm each), unlocks `libc::getpid()`. |
| **P7** | per-process POSIX fd table | `not-started` | Currently the POSIX fd→channel mapping lives entirely in userspace as `hnxlibc::LIBC_FILES[16]` (`hnxlibc/src/lib.rs:263`). An EL0 program that does **not** link hnxlibc (raw OHLINK, raw `__NR_open`) has nowhere to look up the channel associated with a kernel fd. **0.6-α target:** kernel owns the per-process table indexed by `(process_id, fd) → LibcFile { session_chan, remote_fd }`. hnxlibc drops its local table in favour of the kernel one. Page-allocated per-process, like the existing `Process` root VMAR. |

## Bucket 2 - POSIX Libc Names (link-level surface)

| # | Item | State | Notes |
|---|------|-------|-------|
| **L1** | `libc::open` C-ABI | `code-ready-not-verified` | `hnxlibc::open` (`userspace/hnxlibc/src/lib.rs:313`) is `#[no_mangle] pub extern "C"`. Body bypasses `__NR_open` and goes straight through `channel_lookup("svc.vfs")`. Bypasses any raw `syscall(__NR_open, ...)` caller. **0.6-α target:** hnxlibc calls `sys_open`-via-syscall ABI so EL0 programs that link hnxlibc AND EL0 programs that raw-syscall both work. |
| **L2** | `libc::read / write / close / lseek` C-ABI | `code-ready-not-verified` | Same C-ABI shape as L1. **0.6-α target:** same. |
| **L3** | `libc::execve(2)` C-ABI | `working` | `sys_exec` accepts a NUL-terminated path pointer and parsed OHLINK bytes; phase 5.2 wired this up. C name `execve` is exposed in hnxlibc. |
| **L4** | `libc::wait / waitpid` | `not-started` | `syscall/numbers.rs` doesn't define these. Dead process zombies never reaped. **0.7 target:** wrap a PID-collection side channel on `sys_process_exit`. |

## Bucket 3 - ELF / loader / dynamic linking

| # | Item | State | Notes |
|---|------|-------|-------|
| **E1** | loader / runtime search path | `code-ready-not-verified` | Currently `tools/xtask/src/run.rs` and `userspace/services/init/src/main.rs` reference binaries by name (the loader spawns `devmgr`, `fileagent` etc. by literal OHLINK segment name, not by `/bin/devmgr` path). The boot path is `OHLINK → loader executes by binary name`. **POSIX target:** introduce a `/bin/` convention plus loader falls back to filesystem via fileagent when the binary is not in the boot OHLINK manifest. **0.7 target.** |
| **E2** | dynamic linker (`ld.so`) | `not-started` | All current userland is statically linked via OHLINK. `dlopen` not in scope. **1.0 target (optional).** |

## Bucket 4 - Filesystem model

| # | Item | State | Notes |
|---|------|-------|-------|
| **F1** | fileagent `RamFs` | `working` | fileagent `userspace/services/fileagent/src/main.rs` (410 lines): full VFS-on-channel service backing `svc.vfs`; pre-mounts `welcome.txt`. Brings up via `sys_channel_register("svc.vfs")`. |
| **F2** | persistent rootfs | `not-started` | `kernel/src/rootfs.rs::ROOTFS_IMAGE = include_bytes!("../files/rootfs.img")` embeds the rootfs in kernel binary for fileagent to `mmap`/`read`. No on-disk persistence in QEMU. **1.0 target:** SD / eMMC / ATF model. |
| **F3** | device files (`/dev/null`, `/dev/console`, …) | `not-started` | fileagent only mounts `RamFs`; no special-file table. **1.0 target.** |

## Bucket 5 - Signal

| # | Item | State | Notes |
|---|------|-------|-------|
| **S1** | `sys_event_create / signal / ack` | `not-started / PosixBroken` | `SYSCALL_EVENT_*` defined (70/71/72, AUDIT M1) but no dispatch arm. No EL0 caller anyway because no libc signal wrapper exists. **0.7 target:** signal-compatible wrapper around the kernel `Event` object (`kernel/src/ipc/event.rs`). |

## Bucket 6 - I/O multiplex

| # | Item | State | Notes |
|---|------|-------|-------|
| **I1** | `sys_port_create / wait / queue` | `code-ready-not-verified` | `Port` in `kernel/src/ipc/port.rs` is fully implemented (M14). SYSCALL numbers 20/21/22 are defined but no dispatch. **0.7 target:** wire dispatch + `libc::poll / select / epoll`-style wrappers. |

## Bucket 7 - Process model

| # | Item | State | Notes |
|---|------|-------|-------|
| **Pr1** | `sys_spawn` | `working` | Used by loader T10/T30/T70 chain. |
| **Pr2** | `sys_exit` | `working` | Fixed in `4aace5e` — Dead + reschedule. |
| **Pr3** | `fork(2)` | `not-design (fork-less posix — K-D1)` | See top-of-document knob. Every program starts from a clean `Process::new`. `posix_spawn` is the replacement. |
| **Pr4** | `sys_exec` (execve) | `working` | OHLINK-segment-aware. Accepts NUL-terminated path + bytes pointer. |

## Bucket 8 - Memory model

| # | Item | State | Notes |
|---|------|-------|-------|
| **M1** | `sys_mmap` / `sys_munmap` | `not-started` | VMAR syscall surface exists conceptually (M1 SYSCALL_VMAR_*), but no POSIX `mmap`-compatible ABI. **0.7 target.** |
| **M2** | `sys_brk` / `sbrk` | `not-started` | Not in scope. |

## Bucket 9 - Kernel safety

| # | Item | State | Notes |
|---|------|-------|-------|
| **K1** | `validation.rs` no-op facade (AUDIT M8) | `latent` | `validate_pointer / validate_mut_pointer / validate_buffer` all `return Ok(())` with no address-range or rights check. `sys_write` in `handlers/mod.rs:42` does `core::ptr::read` on `kernel_va` after per-byte `translate_user_va` — the validation layer above does nothing. **0.6-α target:** rewrite `validation.rs` to do real user-VA range + rights checks; route every forwarder through it before `safe_copy_*`. |
| **K2** | `KernelObject::duplicate` (AUDIT M4) | `latent` | Only `Vmo` can be duplicated today. POSIX `dup(2)` would need cross-handle-table `Channel + Vmar + Thread + Process + Port` duplication. **0.7 target.** |
| **K3** | `alloc_page` non-atomic (AUDIT M9) + scheduler lock + IRQ-on ordering (H5) | `latent` | `static mut NEXT_FREE_PAGE / END_FREE_PAGE / FREE_PAGES_COUNT` under a `compare_exchange_weak` lock that **enables IRQs on release**. The two are coupled — fixing K3 without H5 still races. **0.6-α target:** convert `alloc_page` to `AtomicUsize` and switch `Scheduler::lock/unlock` to a save-PSTATE-with-IRQs-disabled spinlock (aarch64: `mrs daif; msr daifset, #f; …; msr daif, saved`). |
| **K4** | `static mut FUTEX_TABLE` (AUDIT M10) | `latent` | Same class of race as K3. **0.7 target** (after futex syscall lands). |
| **K5** | global `HANDLE_TABLE_LOCK` (AUDIT M5) | `latent` | One global lock serialises handle ops across all `HandleTable`s. **0.7 target.** |

---

## Section A - Carry-over from AUDIT (parked / latent, not POSIX)

| # | Item | State | Notes |
|---|------|-------|-------|
| **A1** | RISC-V SV39 + boot_asm.S hi-split (H1-H3, TODO L2) | `parked` | rust-lld mis-splits `auipc/addi/jalr` in `boot_asm.S`. Pre-MMU hang. Per Phase-5.5 plan and K-D3, RISC-V path is parked; reopens as a future rein. |
| **A2** | EL0-FAULT EC=0x24 during `sys_spawn(devmgr)` | `code-ready-not-verified` | `6f7f345` (cache-coherent PTE writes + post-shatter flush) merged. QEMU re-run still shows the fault at loader T10. Independent root-cause investigation continues under Phase 5.5 follow-up. Root cause suspected to be QEMU-TCG aarch64 store-ordering that pure cache-flush barriers cannot eliminate. |

## Section B - Test harness (long-term investment)

| # | Item | State | Notes |
|---|------|-------|-------|
| **T1** | host-side unit tests | `not-started` | CHANGELOG `Planned for 0.5.10+` acknowledges this; user decision was: **post-0.6 investment**. |
| **T2** | fault-injection harness (SError / EC=0x24) | `not-started` | SError EL0 handler in `bf10109` is code-ready but never verified by fault injection. T2 would let us tick the "code-ready-not-verified" tag off A2. |

---

## Section C - Phase 6 v0.6.0-α scope lock

**In scope for 0.6-α** (POSIX surface, fork-less, AArch64-only):

1. P1 `sys_open` → forwarder
2. P2 `sys_read` fd≥3 → forwarder
3. P3 `sys_write` fd≥3 → forwarder
4. P4 `sys_close` → forwarder
5. P5 `sys_lseek` → forwarder
6. P6 `sys_getpid` + `sys_gettid` → wire
7. P7 per-process POSIX fd table (kernel-owned)
8. K1 `validation.rs` → real range + perms check
9. K3 `alloc_page` atomic + H5 scheduler lock fix (paired)
10. A2 EL0-FAULT T10 root-cause (alongside, since the work overlaps with `sys_spawn` handler refactor)

**Out of scope for 0.6-α** (move to 0.7 / 1.0):

- L4 `libc::wait / waitpid`
- E1 loader search-path convention
- F2 / F3 persistent rootfs + device files
- S1 signal syscalls
- I1 port syscalls (`poll/select/epoll`)
- M1 / M2 `mmap/brk` syscalls
- K2 / K4 / K5 handle-table + futex + global-handle-lock atomics (K3 is in-scope; H5 too)
- T1 / T2 test harness
- A1 RISC-V path
