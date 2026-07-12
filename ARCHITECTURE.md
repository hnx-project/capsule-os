# Architecture

> **Audience:** a developer who can already build and run CapsuleOS
> (see [`README.md`](./README.md)) and now wants to understand the
> code as a system — where each layer ends, where the boundaries
> between privileged and user-mode code live, and which file owns
> which invariant.
>
> **Complementary documents:**
> * [`README.md`](./README.md) — what the project is, how to build,
>   how to run, status, license.
> * [`AGENTS.md`](./AGENTS.md) — engineering rules for AI agents and
>   human contributors (branching, commits, the `xtask` toolchain,
>   OHLINK format, monorepo topology).
> * [`TODO.md`](./TODO.md) — phase-by-phase roadmap with
>   checkboxes.
> * [`CHANGELOG.md`](./CHANGELOG.md) — release-by-release history.

---

## 1. Layered architecture

CapsuleOS is a strict **four-layer** system.  Each layer is allowed
to depend on the layer below it and on itself; it must **not**
depend on anything above.

```
┌──────────────────────────────────────────────────────────────────┐
│  L4  User applications (hnx-init, hnx-loader, hnx-osh, ls, …)    │
│      EL0 / U-mode.  No MMIO, no privileged instructions, no      │
│      raw pointer passing across the kernel boundary.  All        │
│      resource access goes through `Handle`s.                     │
├──────────────────────────────────────────────────────────────────┤
│  L3  System services (devmgr, fileagent, future vfs)             │
│      EL0 / U-mode.  Same rules as L4 but considered part of     │
│      the OS's "always-on" surface.  A bug here is a bug in the  │
│      product, not in user code.                                  │
├──────────────────────────────────────────────────────────────────┤
│  L2  Userspace support crates                                    │
│        hnxlibc   C-ABI shim over the syscall surface             │
│        hnxstd    #![no_std] standard library (Vec, String, …)   │
│      EL0 / U-mode.  Pure library code; no `fn main`.            │
├──────────────────────────────────────────────────────────────────┤
│  L1  HNX microkernel  (hnx-core)                                 │
│      EL1 (AArch64) / S-Mode (RISC-V 64).  The only privileged   │
│      code.  Owns the MMU page tables, the scheduler, the GIC /  │
│      PLIC, the IPC primitives, the `HandleTable`, and the       │
│      syscall dispatch entry point.                               │
├──────────────────────────────────────────────────────────────────┤
│  L0  capsule-bootloader                                          │
│      AArch64 EL3 / RISC-V 64 M-mode.  Brings up CPU 0, parks    │
│      the other cores in `wfi`, decodes the OHC container,       │
│      decompresses the kernel image, and jumps to the kernel     │
│      entry point with the FDT pointer in `x0`.                   │
└──────────────────────────────────────────────────────────────────┘
```

A handy one-liner for newcomers:

> "If your code runs with interrupts masked and the MMU pointing
> at kernel page tables, you are in L1.  If it runs with the MMU
> pointing at a per-process page table and you got there by
> `eret`, you are in L2–L4."

---

## 2. Repository topology

```
capsule-os/                          (this repo, GitCode)
├── README.md                        public-facing intro
├── AGENTS.md                        engineering rules (AI + human)
├── CHANGELOG.md                     release history
├── TODO.md                          phase roadmap
├── LICENSE                          placeholder, not yet selected
├── xtask.toml                       global build / project metadata
│
├── install_xtask                    bootstrap script (host)
├── build/                           host-side build artefacts (gitignored)
│   ├── target/                      cargo intermediate artefacts
│   └── dist/                        xtask emit (kernel/qemu.dtb live here)
│       ├── kernel/                  kernel.elf, kernel.raw, hnxcore
│       ├── qemu.dtb
│       ├── staging_rootfs/          rootfs components
│       └── distribution/            final capsuleos-pangu-*.img (per-version)
│
├── bootloader/                      subtree: capsule-bootloader    (L0)
├── kernel/                          subtree: hnx-core              (L1)
│   ├── linker/                      per-arch linker scripts
│   └── src/
│       ├── arch/                    per-arch HAL (aarch64/, riscv64/)
│       ├── mm/                      VMO / VMAR / page allocator
│       ├── task/                    Process, Thread, Scheduler
│       ├── object/                  HandleTable, capability rights
│       ├── ipc/                     Channel, Port
│       ├── syscall/                 syscall numbers + dispatch table
│       ├── drivers/                 GIC, PL011, generic timer
│       └── lib.rs                   kernel_main + smoke tests
│
├── userspace/                                                   (L2–L4)
│   ├── hnxlibc/                     C-ABI shim
│   ├── hnxstd/                      #![no_std] stdlib
│   ├── services/                    L3 system services
│   └── programs/                    L4 apps
│
├── std/                             custom target JSON spec files
│   └── targets/                     aarch64-unknown-capsule.json,
│                                    riscv64-unknown-capsule.json
│
├── tools/                           host-side toolchain
│   ├── ohlink-cc/                   subtree: ohlink-cc (compiler
│   │                                backend, linker, VM emulator)
│   └── xtask/                       `xtask code ...` / `xtask repo ...`
│
└── .harness/                        4-rein AI dev team (orchestrator
                                     + 4 reins; see `.harness/agent.md`)
```

Subtrees vs. submodules: CapsuleOS uses **subtrees** for the three
external projects (`capsule-bootloader`, `hnx-core`, `ohlink-cc`).
Subtrees are a single linear history; submodules were a constant
source of "I cloned but `kernel/` is empty" pain.  The downside is
that subtrees make cherry-picking awkward, so cross-project patches
go through the `*-up` remotes and a `xtask repo pull` cycle rather
than a direct submodule pointer bump.

---

## 3. Boot sequence (QEMU → first user-space print)

```
QEMU (aarch64 virt, -cpu max, -machine virt,gic-version=3)
  │
  │  resets to EL3, jumps to capsule-bootloader L0 entry
  ▼
L0 capsule-bootloader
  │  - masks all interrupts
  │  - parks secondary cores in `wfi`  (single-CPU bring-up)
  │  - parses the OHC container at flash offset
  │    - validates the OHLK magic and CRC32-IEEE checksum
  │    - copies the hnx-core payload to 0x4008_0000
  │  - passes the FDT physical address in x0
  │  - eret into EL1
  ▼
L1 hnx-core,  boot_asm.S
  │  - allocates the boot stack (linker symbol __boot_stack_top)
  │  - zeroes .bss
  │  - stashes dtb_ptr in x19, restores it to x0, bl kernel_main
  ▼
kernel_main  (kernel/src/lib.rs)
  │  - early_init()   PL011 UART up so we can log
  │  - MMU bootstrap    4-level page tables, identity + high-half
  │  - GIC + generic-timer init
  │  - vmo_vmar_smoke_test()    boot-time self-check
  │  - Process::launch_user_program("loader", …)
  │    which schedules the first user-space thread
  │  - SCHEDULER.run()  never returns
  ▼
EL0 user mode
  │  hnx-loader (pid 1) starts
  │  - "Loader service launched successfully at EL0!"
  │  - "T10: about to call syscalls::spawn(devmgr)"
  │  - syscalls::spawn("devmgr", …)
  │    → EL0 SVC → aarch64_sync_el0_handler → sys_spawn
  │    → Process::launch_user_program("devmgr", …)
  │    → kernel marks caller Dead, calls schedule()
  │    → switch_to into devmgr's TrapFrame
  ▼
EL0 user mode
     hnx-devmgr (pid 2) starts
     - "CapsuleOS device manager starting…"
     - "PL011 UART driver initialized"
     - "device manager running"
     - sys_exit(0)
     → kernel marks caller Dead, calls schedule()
     → no other runnable threads → "No runnable threads left!
       Halting CPU safely…"
```

The branch that landed in 0.5.9 (`4aace5e fix(syscall)`) is what
turned that final `Halting CPU safely…` from a `SCHED-SAME` dead
loop into a clean kernel halt.

---

## 4. Runtime data structures

All of these live in `kernel/src/`.  Knowing the invariants is
worth more than reading every `impl` block.

### `Process` (`task/process.rs`)
* Owns a `VMAR` (the root virtual address space).
* Owns a `HandleTable` (every resource visible to the process is
  addressed by a `u32` `Handle` value).
* Has an integer `id` (the user-space-visible "pid").  `pid 1` is
  the boot anchor (see §7).
* Has a name (`&'static str` for now; an `intern_name` slot table
  in `syscall/handlers/process.rs` interns user-supplied names).
* Has a per-process `l0_user_pa` (the L0 page-table physical
  address) used by `safe_copy_from_user` / `safe_copy_to_user`
  for syscall argument marshalling.

### `Thread` (`task/thread.rs`)
* Belongs to a `Process` via `process_id`.
* Has a `state`: `Ready / Running / Blocked / Sleeping / Dead`.
* Has a `priority` (`Priority` enum, 5 levels), with a time-slice
  counter that decays priority on starvation.
* Has a `context: ThreadContext` (callee-saved registers, `SP`,
  `LR`, `ELR_EL1`, `SPSR_EL1`, `SP_EL0`) — what `switch_to` loads
  on resume.
* Has an `id` (the user-space-visible "tid", not the scheduler's
  internal `slot index`).

### `VMO` (`mm/vmo.rs`)
* 4 KiB-granular virtual memory object.
* Lazy allocation: `read` on an uncommitted page returns 0.
* `commit_page` / `commit_all` for explicit population.
* Reference-counted; the last `unmap` drops it.

### `VMAR` (`mm/vmar.rs`)
* Tree of virtual address ranges (`root` + sub-regions).
* Each `VMAR` has a `VmarFlags` bit-set (`READ / WRITE / EXECUTE
  / MAP`).
* `map` installs 4 KiB page-table entries one at a time, shattering
  L1 1 GiB and L2 2 MiB blocks on demand.
* `unmap` reverses the operation; `protect` flips flags in-place.

### `Channel` (`ipc/channel.rs`)
* Synchronous rendezvous (zero-allocation).
* Static `send_waiters` and `recv_waiters` FIFO queues.
* Direct handoff: the kernel copies payload bytes from the
  sender's kernel stack directly into the receiver's kernel stack
  at rendezvous, then transfers any `Handle`s from the sender's
  table to the receiver's table.

### `Port` (`ipc/port.rs`)
* 32-byte async completion port.
* Ring buffer in a dedicated page (`Port::ring_pa`).
* Per-TCB `port_packet_slot` (one slot reserved per thread that
  has ever called `Port::wait`); no per-event allocation.

### `HandleTable` (`object/handle_table.rs`)
* Sparse `u32` → `KernelObject` mapping per process.
* `KernelObject` is a tagged enum: `Vmo(u64)`, `Vmar(u64)`,
  `Channel(u64)`, `Port(u64)`, `Thread(u64)`, `Process(u64)`.
* Rights are an explicit `Rights` bitset decoded at every
  syscall boundary — there is no implicit "if you have the handle
  you can do anything".
* **No raw pointers cross the kernel boundary.**  This is a
  non-negotiable invariant of the project (see
  `AGENTS.md` "Handle Isolation" and the
  `safe_copy_from_user` / `safe_copy_to_user` helpers in
  `syscall/handlers/ipc.rs`).

---

## 5. Trap and system call dispatch

AArch64 has 16 vector slots; the kernel wires up **3** of them for
real work and 13 of them to "diagnostic character + halt" stubs:

| Slot offset | Class                 | Wired up?            | Handler                          |
| ----------- | --------------------- | -------------------- | -------------------------------- |
| `0x000`     | Sync,   EL1, SP_EL0  | no (halt)            | `sync_el1_sp0`                   |
| `0x080`     | IRQ,    EL1, SP_EL0  | no (halt)            | `irq_el1_sp0`                    |
| `0x100`     | FIQ,    EL1, SP_EL0  | no (halt)            | `fiq_el1_sp0`                    |
| `0x180`     | SError, EL1, SP_EL0  | no (halt)            | `serror_el1_sp0`                 |
| `0x200`     | Sync,   EL1, SP_ELx  | no (halt)            | `sync_el1_spx`                   |
| `0x280`     | IRQ,    EL1, SP_ELx  | **yes (timer tick)** | `irq_el1_spx` → `irq_handler`    |
| `0x300`     | FIQ,    EL1, SP_ELx  | no (halt)            | `fiq_el1_spx`                    |
| `0x380`     | SError, EL1, SP_ELx  | no (halt)            | `serror_el1_spx`                 |
| `0x400`     | Sync,   EL0, 64-bit  | **yes (SVC + fault)**| `sync_el0` → `aarch64_sync_el0_handler` |
| `0x480`     | IRQ,    EL0, 64-bit  | **yes (timer tick)** | `irq_el0` → `irq_handler`        |
| `0x500`     | FIQ,    EL0, 64-bit  | no (halt)            | `fiq_el0`                        |
| `0x580`     | SError, EL0, 64-bit  | **yes (since 0.5.9)**| `serror_el0` → `aarch64_serror_el0_handler` |
| `0x600-0x780`     | EL0, 32-bit | no (halt)            | all four `*_el0_32` aliases        |

Each wired-up stub follows the same template:

1. `sub sp, sp, #208` (allocate the `TrapFrame`).
2. `mov x19, sp` (stash the frame pointer; `x19` is the only
   callee-saved register the dispatcher can rely on surviving
   nested traps, see `9a47ac7`).
3. Save `x0..x18` and `x30` (LR).
4. Save CSRs: `spsr_el1`, `elr_el1`, `esr_el1`, `far_el1`.
5. Save `sp_el0` (so the `eret` epilogue restores the user SP,
   not the kernel SP the stub `sub`'d).
6. `bl` the Rust dispatcher.
7. Restore CSRs (with a double-write of `elr_el1` around an
   `isb` to work around a QEMU TCG quirk; see the comment at
   `boot_asm.S:240`).
8. Restore `x0..x18`, `x30`, `sp_el0`.
9. `add sp, sp, #208`; `eret`.

The `aarch64_sync_el0_handler` (in `trap.rs`) dispatches on
`ESR_EL1.EC`:

* `0x11` / `0x15` → SVC: the `syscall_dispatch` table in
  `syscall/mod.rs` looks up the `SYSCALL_*` number in `x16` and
  invokes the matching handler.  `elr += 4` after dispatch so the
  `svc` instruction is skipped on `eret`.
* Everything else (data abort `0x24`, instruction abort `0x20`,
  etc.) is a fault: log `EL0-FAULT`, mark the current thread
  `Dead`, call `SCHEDULER.schedule()` so the kernel swaps to the
  next runnable thread instead of `eret`-ing back into the
  faulting context.

The `aarch64_serror_el0_handler` is structurally identical to the
non-SVC branch of `aarch64_sync_el0_handler`, but it also decodes
the `AET` field (`ESR_EL1.ISS[12:10]`) so the log distinguishes
"uncategorised" from "uncontainable" SError sources.

---

## 6. Process lifecycle

```
         Process::launch_user_program()
                  │
                  ▼
   ┌──────────── create Process ─────────────┐
   │  allocate Process slot in PROCESSES     │
   │  build root VMAR, install page table    │
   │  call into sys_exec's OHLINK parser     │
   │  → 0x11 (SVC) or 0x15 (SVC) by syscall │
   └─────────────────────────────────────────┘
                  │
                  ▼
            create Thread
   ┌──────── add to scheduler ───────────────┐
   │  SCHEDULER.add(thread)                  │
   │    - find empty slot in threads[]       │
   │    - push slot index into priority queue│
   │    - state = Ready                      │
   └─────────────────────────────────────────┘
                  │
                  ▼
        ┌──── Running ────┐
        │  trap → EL1     │
        │  schedule()     │ ←──┐
        │  switch_to(…)   │    │ Ready
        └─────┬───────────┘    │ queue
              │                │
              ▼                │
        ┌──── Blocked / Sleeping (channel / port / wait)
        │  wake_thread() re-queues
        └─────────────────────────┘

   End-of-life paths:
     • voluntary:   thread runs sys_exit → marked Dead, reschedule
     • fault:       aarch64_sync_el0_handler non-SVC branch → Dead
     • SError:      aarch64_serror_el0_handler → Dead
     • parent kill: future (not yet implemented; the TCB-level
                    `ThreadState::Dead` flag is set by the killer
                    and the scheduler will reap the slot on the
                    next pop_next pass)
```

### Init anchor (pid 1)
The first user-space process launched by the kernel (currently
`hnx-loader`) is `pid 1` and is the **boot anchor**: when it
dies, the kernel is responsible for bringing the system back to a
runnable state.  As of 0.5.9, the system gracefully halts in
`SCHED No runnable threads left! Halting CPU safely…` when the
anchor dies.  Init anchor respawn (the kernel automatically
spawning `system/bin/init` when pid 1 exits) is queued for 0.5.10
or 0.6.0; see `TODO.md` Phase 5.5 follow-up.

---

## 7. Dual-architecture symmetry

Every per-arch source file in `kernel/src/arch/` has a sibling in
the other arch directory.  The mapping is **not always one-to-one
in line count** (AArch64 has more historical code), but the public
contract from `arch::*` is the same.

| Concern                          | AArch64 file                          | RISC-V 64 file                          |
| -------------------------------- | ------------------------------------- | --------------------------------------- |
| Vector table, trap entry         | `arch/aarch64/boot_asm.S`             | `arch/riscv64/boot_asm.S`               |
| EL1 trap dispatcher (Rust)       | `arch/aarch64/trap.rs`                | `arch/riscv64/trap.rs`                  |
| MMU (4-level / SV39)             | `arch/aarch64/mmu.rs`                 | `arch/riscv64/mmu.rs`                   |
| UART early init                  | `arch/aarch64/console.rs`             | `arch/riscv64/console.rs`               |
| GIC / PLIC driver                | `drivers/gic.rs` (AArch64-only)       | `drivers/plic.rs` (RISC-V-only)         |

The contract that "lives above the arch line" is in `arch/mod.rs`:

```rust
pub mod trap;          // re-exports per-arch TrapFrame
pub mod mmu;           // re-exports MapFlags + MapPage
pub mod console;       // re-exports putchar / puts
```

When you change the contract, every arch has to update.  When you
change an implementation, only one arch updates and the contract
file is untouched.  The harness's `aarch64-expert` rein is
explicitly told to leave a `riscv64 impact:` comment on any change
to the AArch64 side, because the RISC-V 64 HAL has no dedicated
owner today (see `README.md` "Known Limitations").

---

## 8. Debugging and observability

### Log tags
Every kernel log line is columnar:

```
INFO  | TRAP            | EL0 Trap Intercepted! ESR=0x92000007 ELR=0x90213118
ERROR | EL0-FAULT       | EC=0x24 ... thread=#1 -- KILLED ...
INFO  | SYSCALL         | Process exited with code 0
ERROR | SCHED           | No runnable threads left! Halting CPU safely...
```

| Tag               | What it means                                         |
| ----------------- | ----------------------------------------------------- |
| `BOOT`            | Bootloader and kernel start-of-day                    |
| `FDT`             | FDT hardware parsing                                  |
| `MM`              | Physical page allocator                               |
| `MMU`             | Virtual memory (page-table walks, map, unmap)         |
| `IRQ` / `GIC`     | Interrupt controller setup / acknowledge              |
| `SMOKE`           | Boot-time self-check                                  |
| `ROOTFS`          | OHLINK image / rootfs lookup                          |
| `LAUNCHER`        | `Process::launch_user_program`                        |
| `SCHED`           | Scheduler add / switch / same-thread short-circuit    |
| `SCHED-SAME`      | The `prev == next` short-circuit fired (usually a no-op) |
| `SYSCALL`         | Syscall entry / exit summary                          |
| `SVC-PRE`         | Just before the SVC dispatch table is consulted       |
| `EL0-FAULT`       | Sync EL0 fault (instruction abort, data abort, etc.)  |
| `SError-FAULT`    | Async SError from EL0 (since 0.5.9)                  |
| `EXEC` / `SPAWN`  | `sys_exec` / `sys_spawn` outcomes                     |
| `GETCWD` / `CHDIR` | `sys_getcwd` / `sys_chdir` outcomes                  |
| `LOAD_BINARY`     | `sys_load_binary` (VMO-backed exec)                   |

### QEMU + GDB

```bash
# Terminal 1: launch with GDB stub
xtask code run --arch aarch64 --gdb &

# Terminal 2: attach gdb-multiarch
gdb-multiarch build/dist/kernel/kernel.elf \
    -ex "target remote :1234" \
    -ex "hbreak kernel_main" \
    -ex "continue"
```

`kernel.elf` has full symbols (rustc `-C debuginfo=2`); set
breakpoints on any kernel function by name.

### QEMU monitor

```bash
# kill QEMU and dump a backtrace from the host side
Ctrl-A C       # enter QEMU monitor
info registers
info mmu
gpa2hva 0x92003d68
```

### Panic stack
A kernel panic prints the file:line, the trap class, `ESR`, `ISS`,
`ELR`, and `FAR`.  Walk the printed frame by feeding `ELR - 4`
into `addr2line -e build/dist/kernel/kernel.elf` to get the source line.

---

## 9. Where to read next

| If you want to…                                       | Read                                       |
| ----------------------------------------------------- | ------------------------------------------ |
| Add a new system call                                | `kernel/src/syscall/numbers.rs` + `mod.rs` |
| Add a new EL0 process (L4 app)                        | `userspace/programs/<name>/src/main.rs`    |
| Add a new L3 system service                           | `userspace/services/<name>/`               |
| Change a capability / `HandleTable` invariant         | `kernel/src/object/handle_table.rs`        |
| Change the scheduler                                  | `kernel/src/task/scheduler.rs` + `thread.rs` |
| Change the OHLINK parser                             | `tools/ohlink-cc/ohlink-format/`            |
| Add a new trap class (e.g. wire up `fiq_el0`)        | `kernel/src/arch/<arch>/boot_asm.S`        |
| Tweak the boot sequence                              | `kernel/src/lib.rs::kernel_main`           |
| Update the public API surface (libc / std)            | `hnxlibc/` + `hnxstd/` |
| Tweak the `xtask` workflow                           | `tools/xtask/src/`                         |
| Change the dual-arch contract                        | `kernel/src/arch/mod.rs` + per-arch sibling |
