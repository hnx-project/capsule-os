# `kernel::smp` — Symmetric Multiprocessing Subsystem

**Status**: Active (使用中)
**Owner**: kernel/scheduler; boot path

This document describes the SMP bootstrap and topology layer that
the Pangu 1.0 microkernel uses to bring up the secondary CPU
cores on an `aarch64` system, and the invariants shared with
`kernel::task::scheduler` to keep the per-core runqueues sane.

---

## Core Definition

`kernel::smp` is the **single source of truth** for "is this CPU
slot alive, and which slot am I on?".  It mirrors the Linux
kernel's `cpu_possible_mask` / `cpu_present_mask` /
`cpu_online_mask` triplet:

| Mask                  | Meaning                                                          |
|-----------------------|------------------------------------------------------------------|
| `POSSIBLE_MASK`       | firmware declared a `cpu@N` in `/cpus` for this slot             |
| `PRESENT_MASK`        | `POSSIBLE_MASK` ∩ {compatible string starts with `arm,cortex`}   |
| `ONLINE_MASK`         | slot has confirmed it is executing kernel code                   |

A core slot (`0..MAX_CORES = 8`) is considered "under kernel
control" iff `ONLINE_MASK[slot] == true`.  The scheduler's
`current_indices[slot]` is only meaningful for an `ONLINE_MASK`
slot; the timer-tick path skips ticks on cores that have not
been registered yet.

## Probe sequence (DTB → PSCI)

The bring-up happens in `boot_secondary_cores()` in two passes:

1. **FDT pass** (`probe::pass_fdt`): walks `/cpus` via the shared
   `kernel::fdt::scan_cpus_node` API, harvesting
   `reg / enable-method / status` for every `device_type = "cpu"`
   child.  Sets `POSSIBLE_MASK[reg]` for each.  If the DTB is
   missing or has no `/cpus` node, only slot 0 is set.
2. **PSCI pass** (`probe::pass_psci`): for every `POSSIBLE_MASK`
   bit, issues `PSCI_AFFINITY_INFO(slot, ON)` and translates the
   SMC return value through `psci_alive(...)`.  A slot becomes
   `ONLINE_MASK` if the firmware answers `RUNNING | ALREADY_ON`.
   A `PENDING` answer is retried after a brief spin.

`pass_fdt` is synchronous and **must** be called from
`kernel_main` before `boot_secondary_cores()` issues any
`PSCI_CPU_ON`.

## Secondary entry

`kmain_secondary(slot: usize)` in `per_core.rs` is the
assembly-trampoline target.  Its contract is:

1. `smp::register_core(slot)` — store the slot in
   `CURRENT_CORE_SLOT`, increment `BOOTED_CORES`, and force
   `ONLINE_MASK[slot]`.
2. Bring up the per-core GIC CPU interface and generic timer.
3. Enable IRQs (`enable_irqs`).
4. Enter `SCHEDULER.schedule()` until the kernel is shut down.

The mailbox layout (`SECONDARY_CORE_ENTRY`, `SECONDARY_CORE_SP`)
is defined in `arch/aarch64/boot_asm.S`.  `boot_secondary_cores()`
writes both before `PSCI_CPU_ON`, and `kmain_secondary` clears
`SECONDARY_CORE_ENTRY` once it has reached the trampoline so the
primary CPU can `spin-wait` for the handshake.

## Exposed surface

```rust
/// Soft upper bound on the number of physical CPU slots.
/// The kernel never indexes `current_indices[]` past this.
pub const MAX_CORES: usize = 8;

/// Bitmask of slots the firmware declared as possible.
pub static POSSIBLE_MASK: CpuMask;
/// Bitmask of slots with a known-good compatible string.
pub static PRESENT_MASK: CpuMask;
/// Bitmask of slots that are currently executing kernel code.
pub static ONLINE_MASK: CpuMask;

/// Slot id of the calling CPU.  Falls back to `MPIDR_EL1.Aff0`
/// during the brief window before `register_core` stores it.
pub fn current_core_id() -> usize;

/// Called from secondary entry and from `kernel_main` for slot 0.
pub fn register_core(slot: usize);

/// PSCI 1.1 dispatch (hvc / smc).
pub mod psci {
    pub fn psci_version() -> u32;
    pub fn psci_cpu_on(target_cpu: u64, entry_pa: u64, context_id: u64) -> i64;
    pub fn psci_cpu_off() -> i64;
    pub fn psci_affinity_info(target_affinity: u64, state: AffinityState) -> i64;
    pub enum AffinityState { On = 0, Off = 1 }
    pub enum CoreAlive { Running, Pending, Absent, Other }
    pub fn psci_alive(ret: i64) -> CoreAlive;
}

/// DTB pass + PSCI confirmation.
pub mod probe { pub fn probe_cpus(); }

/// Per-core entry trampoline.
pub mod per_core { pub fn kmain_secondary(slot: usize) -> !; }

/// Orchestrates the wakeup of every ONLINE-MASK-missing slot.
pub mod boot {
    pub fn boot_secondary_cores();
    pub fn log_topology();
    pub fn booted_core_count() -> usize;
    pub fn snapshot_possible() -> u64;
    pub fn snapshot_online()   -> u64;
}
```

## Invariants (must hold at all times)

1. `ONLINE_MASK ⊆ PRESENT_MASK ⊆ POSSIBLE_MASK`.
2. For every `slot ∈ ONLINE_MASK`, `CURRENT_CORE_SLOT` on that
   core equals `slot`.
3. `Scheduler.current_indices[slot]` is `Some(idx)` **iff**
   `Thread.threads[idx].owner_core == Some(slot)` — the
   per-iteration occupancy invariant maintained inside the
   scheduler lock.
4. `register_core(slot)` is called exactly once per slot for
   the lifetime of the system (boot-time only).
5. The boot trampoline (`kmain_secondary`) is called exactly
   once per `PSCI_CPU_ON(slot, …)` that returns success.

## Failure / timeout policy

`boot_secondary_cores()` waits at most ~5·10⁷ spin iterations for
the secondary to clear the `SECONDARY_CORE_ENTRY` mailbox.  A
timeout is logged at `WARN` level and `ONLINE_MASK[slot]` is
**still** set so the scheduler doesn't trip an unexpected
unregistered slot — this is intentional (QEMU TCG does not
always deliver `PSCI_CPU_ON` cleanly on `-M virt,secure=off`,
and the alternative would be to lose the slot from the
topology entirely).

## Diagnostic tools

`boot::log_topology()` is called unconditionally from
`kernel_main` right before `boot_secondary_cores()`.  The
output looks like:

```
[SMP] topology: possible=4 present=4 (online decided at boot)
[SMP] Discovered 4 cpus:
[SMP]   cpu@0 method=psci enabled=true present=true
[SMP]   cpu@1 method=psci enabled=true present=true
[SMP]   cpu@2 method=psci enabled=true present=true
[SMP]   cpu@3 method=psci enabled=true present=true
[SMP] Booting secondary cores — possible_mask = 0xf
[SMP] All secondary cores brought up.  online_mask = 0xf
```

## Dependencies & Related Components

- `kernel::fdt` — `/cpus` traversal (`scan_cpus_node`).
- `kernel::arch::aarch64::boot_asm` — mailbox layout, GIC init.
- `kernel::drivers::gic` — per-core GIC CPU interface.
- `kernel::drivers::timer` — per-core generic timer.
- `kernel::task::scheduler` — `current_indices[]` and
  `Thread.owner_core` form the per-core occupancy invariant.
- `kernel::arch::aarch64::psci` — raw `hvc #0` / `smc #0`
  encoder (re-exported through `kernel::smp::psci`).

## Cross-core idle wake (S10)

`kernel/src/smp.rs::IDLE_FLAGS: [AtomicBool; MAX_CORES]` is a
per-slot wake channel.  The helpers are:

```rust
/// Mark `slot` as having work pending; pairs with
/// `wait_for_idle_signal`.  Issues `sev` on aarch64 so any
/// core parked in `wfe` wakes immediately.
pub fn idle_flags_signal(slot: usize);

/// Clear the local slot's wake flag in preparation for
/// `wait_for_idle_signal`.  Issues `sevl` so the next `wfe`
/// actually parks.
pub fn idle_flags_clear(slot: usize);

/// Spin-wait until `IDLE_FLAGS[slot]` becomes true; returns
/// true if the flag was observed, false on timeout.
pub fn wait_for_idle_signal(slot: usize, max_iters: usize) -> bool;
```

`Scheduler::schedule()` clears the local slot before `wfe`
and polls immediately afterwards so a wake that lands during
the unlock/wfe window is not lost.

## Future work

- **DTB-generated topology map**: persist the parsed `CpuDescriptor`
  array as a snapshot read-only after boot, to avoid relying on
  global `static mut`.
- **PSCI `SYSTEM_OFF` / `SYSTEM_RESET`**: needed for `initd`
  controlled shutdown rather than the current `wfi` loop.

- **S11 cross-core idle wake-up**: replace the optimistic
  `ONLINE_MASK.set(slot)` after handshake timeout with an
  `idle_flags[core]` + `sev` event-channelised handshake.
- **DTB-generated topology map**: persist the parsed `CpuDescriptor`
  array as a snapshot read-only after boot, to avoid relying on
  global `static mut`.
- **PSCI `SYSTEM_OFF` / `SYSTEM_RESET`**: needed for `initd`
  controlled shutdown rather than the current `wfi` loop.
