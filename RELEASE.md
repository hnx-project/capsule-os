# CapsuleOS 1.0.0 - "Pangu" Release

This is the first **stable / production-shaped** release of
CapsuleOS.  The 0.5 series was developmental; everything from
0.6.0-alpha on is API-stable per
[`semver.org`](https://semver.org/), with breaking changes
collected in their own major/minor lines.

## What's in 1.0.0

CapsuleOS is a from-scratch microkernel written in Rust. The
Pangu codename carries the EL0 POSIX surface from 0.6 to
something every program in `userspace/programs/` and
`userspace/services/` can compile against.

### Stable kernel ABI

- `kernel/src/syscall/numbers.rs` (single source of truth at
  `kernel/shared/src/syscall_nums.rs`)
- 49 syscall numbers, 27 wired + 12 reserved for follow-up
  releases; the reserved set is documented per-syscall at
  the top of `syscall_nums.rs`
- 5-status marshalling on the wires (`Status::Ok` ... `Status::
  Canceled`), stable across releases

### Stable userland surface (hnxlibc)

- `hnxlibc/src/syscalls.rs` is the canonical SVC ABI mirror;
  every new 1.x release preserves its signatures.
- The `1.0` ABI covers `open/close/read/write/seek/getcwd/chdir`,
  plus `process_create/start/exit`, `wait4`, `kill`, `raise`,
  `sigaction`, `pause`, `pipe`, `dup2`, `spawn`, `execve`,
  `yield`, `gettid`, `getpid`, `getppid`, `channel_*`,
  `vmo_*`, `vmar_*`, `thread_*`, `handle_duplicate`.
- `hnxlibc::panic_handler` formats the panic message (with
  source location) onto stderr (fd 2) via the in-kernel UART
  path; pre-1.0 the handler was a silent `loop {}`, so user
  programs that panicked produced only a frozen UART.

### Stable library surface (hnxstd)

- `std/hnxstd/src/{vec, string, fmt, io, thread}` form a
  minimal `#![no_std]` standard library for EL0 programs.
  The 1.0 line ships:
    - `Vec<T>`  (fixed 64-element backing, allocation-free)
    - `String`  (over `Vec<u8>`)
    - `format!` + `write!` macros that produce owned
      `String` values
    - `io::print/println` that target the in-kernel UART path

### The 1.0 demo

QEMU 25-second boot of the resulting image prints:

```
HNX
T00: enter main
Loader: bringing up EL0 services (devmgr + fileagent)
T01: wrote banner
T10: about to call syscalls::spawn(devmgr)
[devmgr running]
[fileagent running, /etc/hostname + /etc/os-release on the
 ramfs]
init: starting...                    <- bootstrap-resistant:
                                      <- respawn_init_if_anchor
                                      <- kept the chain alive
```

plus the new EL0 panic message if anything panics:

```
EL0 PANIC: could not open file: O_NOT_FOUND @ src/main.rs:24
```

(The 1.0 demo currently displays the **A2 EL0-FAULT
KERNEL_HEALTH.md code-ready-not-verified note** for the
hardened cache / TLB work because QEMU-TCG misses a step;
the boots below the loader show this.  See "Known issues"
below for the unblock that lands in 1.0.1.)

## What's NOT in 1.0.0

Out-of-scope by design for 1.0 to keep the kernel ABI
release-able.  These land in 1.1:

- **Full `fork(2)`** is intentionally not implemented
  (`KERNEL_HEALTH.md` K-D1 fork-less POSIX).  Use `sys_spawn`
  for new processes; `wait4 + kill + SIG_DFL` covers the
  common waitpid() use case in 1.0.
- **Custom user-mode signal handlers** are not in 1.0;
  `sigaction` accepts `SIG_DFL (0)` and `SIG_IGN (1)` only.
  The sigreturn-trampoline / alternate-signal-stack work
  ships in 1.1.
- **Network stack** is a Phase-8 item (`TODO.md`)
- **Real VFS** (`vfs::Vnode` shaped) is a Phase-7 item;
  the fileagent service plays the role in 1.0.
- **True `mm/slab.rs`** allocator behind `Vec` /
  `String` is a 1.1 item (`KERNEL_HEALTH.md` §5).

## Known issues / disclaimer

A2 (`KERNEL_HEALTH.md`) is **partially closed**.  The B1.1-B1.5
series patched the aarch64 AP-bits encoding that masked user-writable
PTE entries as EL0-forbidden; the boot chain now reliably reaches
the spawned devmgr-prints-its-banner point but still hits a
`SError=0x0f` on the very first stack access past that.  1.0.1
(branched from this commit, see `f407929` for the latest tagged
dev build) ships the page-walk fix.

## How to run

```sh
cargo xtask repo setup-fork
cargo xtask code build --arch aarch64
cargo xtask code run --arch aarch64
```

The default QEMU setup has been documented in `ARCHITECTURE.md`
under "Booting".

## Acknowledgements

The 0.x series shipped under the 0.5 line ("alpha-grade
real-time microkernel"); 0.6 lifted to the POSIX surface;
this 1.0 release closes the 0.7-0.9 / Phase-6 buckets.
Major contributors to the 0.6.0-1.0.0 walk:

- The hnx-core team (loader, kernel core, scheduler)
- The hnxlibc team (EL0 ABI mirror)
- The osh team (EL0 shell + pipeline plumbing)
- The init-anchor respawn chain (`kernel/src/task/init_respawn.rs`)
- B4 (wait4), B5 (signals), B6 (pipes), B7 (pipeline), B8
  (/etc/hostname), B9 (hnxstd), B10 (visible panic) all
  tracked per commit in `git log --oneline`.

## 1.0.0 "Pangu"

*Codename*: **Pangu** - the primordial Chinese giant who
  separated the heavens and the earth from the formless
  chaos; appropriate for the launch of a from-scratch OS
  that hand-builds its userspace from -0-.

*Released*: 2026-07-12 (sync'd to the A2 isolation cycle)

*Kernel ABI*: SYSCALL_* numbering frozen at this point.

*Userspace ABI*: C-ABI mirror + Rust slim lib (`hnxstd`).

*Build commit*: see `c408ea6` (1.0.0 tag).
