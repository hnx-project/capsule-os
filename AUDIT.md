# Kernel Completeness Audit (0.5.9-develop)

> **Generated:** 2026-07-11
> **Scope:** `kernel/src/**` and the supporting `kernel/hal`,
> `kernel/shared` workspace crates.
> **Method:** static analysis of source, cross-referenced against
> the dispatcher table in `syscall/mod.rs::syscall_dispatch`.
>
> This is a *completeness* audit, not a *correctness* audit.  It
> identifies places where the kernel has scaffolding that is not
> connected to the live boot path, plus a small number of
> high-severity reachability issues that gate the 0.6 → 1.0
> promotion.

---

## Headline

CapsuleOS is a working microkernel bring-up whose AArch64 path can
boot QEMU `virt`, build 4-level page tables, run a preemptive
scheduler with IPC channels, and launch OHLINK EL0 user programs.
**The AArch64 side is solid for the MVP; the RISC-V side is
intentionally half-finished and the kernel is honest about it.**
The kernel has accumulated a lot of dead scaffolding: an entire
`hal` crate, a `*Mmu` / `*PageTable` / `*AddressSpace` vtable layer,
a `slab` allocator, a `Mutex`/`Semaphore`/`Event` set, a
`VMAR::protect` half-impl, the `sys_read` "VFS_READ_OK" stub, and a
long list of `ObjectType` / `VnodeType` / `Rights` variants and
syscall numbers with no corresponding handler.  About **30% of
the syscall surface is unimplemented** (port, event, timer, futex,
VMAR unmap/protect, VMO get/set size, process/thread exit,
channel_call).  To reach 1.0 the work is:

1. close the RISC-V MMU `csrw satp` regression so SV39 actually
   translates;
2. wire up the missing syscalls or remove their numbers;
3. delete or actually implement the dead `hal` / `slab` /
   `sync::primitives` modules;
4. turn the stub `sys_read` into a real rootfs-backed read so
   fileagent and the shell can read files.

---

## High-severity findings (must fix before 1.0)

### H1. RISC-V SV39 is never enabled (`arch/riscv64/mmu.rs:379-394`)

The `csrw satp` / `sfence.vma` block at the end of `enable_inner` is
**intentionally commented out** and replaced with a doc-comment
explaining the bug.  The page-table builder, `map_page`,
`unmap_page`, and `MapFlags` are correct, and `mark_mmu_active()`
is still called, but `satp` is never actually written, so the
kernel runs in S-Mode without SV39 translation (i.e.
identity-mapped everywhere).  The author has identified the symptom
(first post-sATP instruction at PC `0x80081cf0` faults) and
suspected causes (mstatus.MPP / menvcfg inherited from OpenSBI, or
sfence.vma ordering) but has not isolated it.

### H2. RISC-V `translate_user_va` is a stub (`arch/riscv64/mmu.rs:443-444`)

```rust
pub fn translate_user_va(_l0_pa: usize, va: usize) -> Option<usize> {
    Some(va)
}
```

Since the MMU isn't even enabled on RISC-V, the kernel can't
actually be doing user→kernel VA translation there, so this stub
never produces wrong data in practice — but it would silently
break the moment SV39 is enabled.

### H3. RISC-V EL0 launch is missing TTBR0 init (`task/process.rs:91-356`)

`launch_user_program_with_argv` is `#[cfg(target_arch = "aarch64")]`
in practice — the AArch64-specific TTBR0 swap is wrapped in
`#[cfg(target_arch = "aarch64")]`, so **RISC-V silently skips the
L0 initialisation**, the per-process page table is allocated but
never installed, and the process runs through whatever TTBR0 the
kernel was using at that moment.

### H4. `sys_read` returns a literal (`syscall/handlers/mod.rs:31-70`)

`sys_read` (fd != 0 path) reads from a hard-coded byte literal
`b"VFS_READ_OK"` — **the kernel does not actually read from the
rootfs**, it just returns the string every time.  The offset is
tracked per-fd so the user can `seek` to skip bytes, but every
read returns the same 11-byte string.  `sys_open` allocates a
fresh `Vnode` per call but never reads the file content.
**Fileagent can't actually serve file contents; the shell's `cat`
and `cp` are broken.**  (Workaround: fileagent has its own
in-RamFS so its endpoints do work for `welcome.txt`; but the
syscall-level `sys_read` is the wrong place to fix this.)

### H5. Scheduler lock + IRQ-on ordering (`task/scheduler.rs:53, 218, 232`)

`Scheduler::lock()` calls `disable_irqs()` and `unlock()` calls
`enable_irqs()`.  This is the **wrong direction** for a scheduler
lock — re-enabling IRQs inside `unlock` while the previous `lock`
is still in scope opens a window where the IRQ handler can fire
and re-acquire a "scheduler" lock the previous lock-holder already
held, but with IRQs on, the `compare_exchange_weak` can spin
against a contended resource without protection.  The actual
saved-state semantics are not modelled (PSTATE isn't saved).

### H6. Thread park address lands in guard page (`task/process.rs:75-77, 155-156, 307-308`)

Hard-coded `0x1usize as u64` is used as a "park address" for dead
threads.  With `0x1` as `elr` the next `eret` will jump to virtual
address 1, which is the NULL page guard — the intent
("non-zero PC to avoid EC=0x0 ELR=0x0") is correct but the value
chosen is in the always-unmapped guard page, so the next context
restore will fault.  Latent — only triggers in a race between a
tick and a `sys_exec`.

### H7. Multi-process entry-VA collision (`task/process.rs:42-68`)

`Process::new` allocates a per-process VMAR at base
`0x9000_0000 + slot * 0x1000_0000` (256 MiB slot), so slot N is at
`0x9N00_0000`.  The OHLINK loader's
`user_entry_offset = header.entry_point & 0x0FFF_FFFF` is then
added to `vmar_base` to get the actual entry VA.  But the offset
0x0FFF_FFFF keeps only the bottom 28 bits — anything that is in
the upper bits of the original `e_entry` (e.g. `0x90xxxxxx` for
loader, `0xA0xxxxxx` for devmgr) gets its slot bits stripped.
The comment in `launch_user_program_with_argv` at lines 196-208
explains the historical bug; the current code now computes
`user_entry = vmar_base + (e_entry & 0x0FFF_FFFF)`, but **the
OHLINK binaries are still linked at addresses like `0x90212788`**
— for `0x90xxxxxx` this works (0x90... & 0x0FFF_FFFF = 0x0xxxxxx),
but the relocation from slot-relative to vmar-relative assumes the
binary was linked at the same slot.  For devmgr at `0xA0xxxxxx`
the user_entry becomes `0x9000_0000 + (0xA0212788 & 0x0FFF_FFFF) =
0x90212788` — same as the loader, which is wrong.  **Multi-process
boot likely collides on slot 0 entry VAs.**  (Mitigated today by
the fact that only the loader runs first; devmgr's "0xa0211a1c" is
coincidentally the same as the slot 0 base because devmgr is
launched into vmar_base 0xA0000000 and the relative offset is the
same as the loader's.  This is fragile.)

### H8. VFS is a skeleton (`vfs/mod.rs:1-251`)

`Vnode`, `VnodeTable`, `FileDescriptor`, `FileDescriptorTable`,
`PathResolver` are all defined.  The `sys_open` syscall allocates
a fresh `Vnode` per call without ever reading the file.

---

## Medium-severity findings (gate 0.7 → 1.0)

### M1. ~18 missing syscall handlers

The following syscall numbers in `syscall/numbers.rs` have **no
handler in `syscall/mod.rs::syscall_dispatch`** and return
`Status::NotAllowed` to the caller:

- `SYSCALL_CHANNEL_CALL = 13`
- `SYSCALL_PORT_CREATE = 20`, `PORT_WAIT = 21`, `PORT_QUEUE = 22`
- `SYSCALL_VMO_GET_SIZE = 33`, `VMO_SET_SIZE = 34`
- `SYSCALL_VMAR_UNMAP = 41`, `VMAR_PROTECT = 42`
- `SYSCALL_THREAD_EXIT = 52`
- `SYSCALL_PROCESS_START = 61`, `PROCESS_EXIT = 62`
- `SYSCALL_EVENT_CREATE = 70`, `EVENT_SIGNAL = 71`, `EVENT_ACK = 72`
- `SYSCALL_TIMER_CREATE = 80`, `TIMER_SET = 81`, `TIMER_CANCEL = 82`
- `SYSCALL_FUTEX_WAIT = 90`, `FUTEX_WAKE = 91`

Also: `SYSCALL_GET_TID = 2`, `SYSCALL_GET_PID = 3` are defined
but have no dispatch arm.

### M2. `Vmar::protect` no-ops via `lookup_pa = None` (`mm/vmar.rs:290-352`)

`Vmar::protect` calls `lookup_pa(va)` first, but **`lookup_pa`
is hard-coded to `return None`**.  Result: `protect` silently
no-ops on every page because the PA can never be found.
Unreachable today (no `SYSCALL_VMAR_PROTECT` handler), but broken
the day someone wires the syscall.

### M3. `mm::vmar::map` leaks page-table entries on overflow (`mm/vmar.rs:243-256`)

When `storage.map_count >= MAX_MAPPINGS`, the function returns
`Err(Status::NoMemory)` *after* the `for i in 0..page_count` loop
has already mapped every page into the page tables via
`arch_mmu::map_page`.  The mapping table is then not updated, so
the VMAR has ghost mappings that aren't tracked.

### M4. `KernelObject::duplicate` only supports Vmo (`object/handle_table.rs:36-42`)

Means a thread cannot duplicate a handle to a `Vmar`, `Channel`,
`Port`, `Process`, or `Thread` — `sys_handle_duplicate` will
fail for any non-VMO handle.

### M5. Global `HANDLE_TABLE_LOCK` shared across per-process tables (`object/handle_table.rs:55-58`)

A single global `HANDLE_TABLE_LOCK: AtomicBool` is shared across
**every** `HandleTable` (each Process has its own `HandleTable`,
but they all share the same lock).  Multi-process apps serialize
all handle-table operations.  Works, but doesn't scale.

### M6. 256-byte IPC cap (`syscall/handlers/ipc.rs:8-63`)

The 256-byte stack-bounded buffer (`heapless::Vec<u8, 256>`) means
any single IPC message larger than 256 bytes will silently
truncate.  Limitation users can hit.

### M7. Concurrent `sys_channel_lookup` collision (`syscall/handlers/ipc.rs:198-246`)

`sys_channel_lookup` creates a new channel pair `(client_chan,
server_chan)`, but then passes `h_server` to the server's
registered service by writing the handle into the server's
`(*server_service_chan_ptr)` directly.  If two clients race to
look up the same service name, both will succeed and both
`h_server` handles get queued into the same service-side channel
— the server has no way to tell which connection came from
which client.

### M8. `syscall/validation.rs` is a no-op facade (`syscall/validation.rs:1-29`)

`validate_pointer`, `validate_mut_pointer`, `validate_buffer`
all take a value and immediately return `Ok(())` — no actual
address-range or user-VA validation is performed.  Any user
pointer that resolves through `translate_user_va` is accepted.
The `safe_copy_from_user` / `safe_copy_to_user` helpers in
`handlers::ipc` do the actual translation, but e.g. `sys_write`
(in `handlers/mod.rs:42`) calls `core::ptr::read` on `kernel_va`
after a per-byte `translate_user_va` — the validation layer
above it does nothing.

### M9. `alloc_page` is non-atomic (`mm/phys.rs:11-15`)

`NEXT_FREE_PAGE`, `END_FREE_PAGE`, `FREE_PAGES_COUNT`,
`TOTAL_PAGES_COUNT`, `MMU_ACTIVE` are all `static mut` with no
atomicity.  They're only updated under the assumption that the
caller holds the scheduler lock or is single-threaded, but
`NEXT_FREE_PAGE++` in `alloc_page` is a non-atomic
read-modify-write.  Works in single-CPU bring-up, would corrupt
under SMP.

### M10. `static mut FUTEX_TABLE` race (`sync/futex.rs:71-72, 99-126`)

`add_waiter` / `remove_waiter` / `wake_from_entry` paths all
touch the table without locking.  Unreachable today (no
`SYSCALL_FUTEX_WAIT/WAKE` handler), but broken the day someone
wires the syscall.

### M11. Per-process L0 init assumes AArch64 (`task/process.rs:117-136`)

The per-process L0 page table is initialised by copying the
kernel's L0 entries.  This means every process sees the kernel's
L1 mappings (high-half, identity) under TTBR0, **and the
process's own VMO mappings get installed on top, so the new
process's user VAs are translated through the new L0**.  The
mechanism is correct on AArch64; on RISC-V it's a no-op
(see H3).

### M12. `RiscV64Mmu::map_page` destroys 2 MiB block content (`arch/riscv64/mmu.rs:259-269`)

When the L2 entry is a leaf (a "2 MiB megapage"), the code
allocates a new L3 page and overwrites the L2 entry to a
non-leaf pointer — but the original 2 MiB content of the L2
page is *lost*.  A comment even admits "we shouldn't see a real
2 MiB megapage at L2 in the current build" but then
unconditionally destroys the existing entry.  Latent — only
triggers if the layout is changed.

### M13. `ThreadContext` layout assert is AArch64-only (`task/thread.rs:40-46`)

The static asserts panic at compile time if the layout drifts.
The asserts assume AArch64 layout — RISC-V has no equivalent,
and `ThreadContext` is shared between both arches, so the layout
is correct for AArch64 and silently wrong for RISC-V.

### M14. Port syscalls unreachable from EL0 (`ipc/port.rs:1-174`)

`Port::new` / `wait` / `wait_with_timeout` / `queue` are complete
and used by the smoke tests, but **no `syscall` number calls
them** — `SYSCALL_PORT_CREATE/WAIT/QUEUE` (20/21/22) are defined
in `syscall/numbers.rs` and have no handler in
`syscall/mod.rs`.  Same for futex (M1).

### M15. Futex never called from a syscall (`sync/futex.rs:1-251`)

`Futex` is fully wired (wait/wake/wake_all/requeue), but **never
called from a syscall handler** — `SYSCALL_FUTEX_WAIT/WAKE`
(90/91) have no handler.  Same root cause as M14.

---

## Low-severity findings (cosmetic / future work)

The audit turned up the following dead-code and consistency
issues.  None of them affect the boot path; all of them are
cleanup work that should be tracked in `TODO.md`:

- **Dead modules**: `kernel/hal` (entire crate), `mm::slab`
  (114 lines), `sync::primitives` (138 lines, `Mutex` /
  `Semaphore` / `Event`), `task::smoke` (165 lines,
  smoke-test code), `vfs::PathResolver`, `kcore::alloc` (with
  a `nullptr`-returning `#[global_allocator]` landmine).
- **Dead functions**: `tick()`, `current_thread_name()`,
  `AArch64Mmu::enable/disable/is_enabled` (trait shell),
  `AArch64PageTable` / `AArch64PageFlags` /
  `AArch64AddressSpace` (trait shell), `arch::console_getchar`,
  `arch::aarch64::console_putbytes`, `AArch64Cpu`,
  `arch::console::Console`, `MappingRequest`, `Vmo::fork` /
  `Vmo::make_cow`, `ElfLoader::is_valid` (always returns `true`),
  `kcore::init`, `kcore::debug_print`, `BootInfo::empty`,
  `Message::new`, `MessageMetadata`, `MessagePriority`,
  `FutexTable::ensure_initialized`.
- **Unused state**: `Scheduler::tick_count` (incremented on
  every `schedule()` but never read), `FutexTable::initialized`
  (set but never checked).
- **Misleading names**: `ThreadQueue::pop_highest_priority`
  pops `items[0]` (FIFO within the queue), not by priority.  The
  outer `pop_next_from_all_queues` does iterate by priority, so
  the effective behavior is priority-FIFO within a tier, but
  the method name suggests it sorts.
- **Duplicated types**: `MessageMetadata` is defined in both
  `kernel/src/ipc/message.rs` and `kernel/shared/src/ipc.rs`;
  `BootInfo` is defined in both `kernel/src/fdt.rs` and
  `kernel/shared/src/boot.rs`.
- **Unused variants**: `VnodeType::{Directory, Device,
  Symlink, Socket, Pipe}` defined but only `File` constructed.
  `ObjectType::{Event, Timer, UserThread, Job, VmObject}`
  defined in `shared/src/types.rs` but unused.
- **7/10 `Rights` constants unused** (`object/rights.rs`): only
  `READ` and `WRITE` are referenced.
- **Unused imports**: `core::ops::Add` in `mm/vmo.rs`,
  `AtomicUsize` in `mm/slab.rs` and `sync/primitives.rs`.

---

## Severity Roll-Up

| Severity | Count | Highlights |
|----------|-------|-----------|
| **High** | 8 | RISC-V `csrw satp` regression; RISC-V `translate_user_va` stub; RISC-V EL0 launch missing TTBR0 init; `sys_read` returns literal `b"VFS_READ_OK"`; scheduler lock + IRQ-on ordering; thread-park `0x1usize` elr lands in guard page; multi-process entry-VA collision in `Process::new`; VFS is a skeleton |
| **Medium** | ~22 | 18 missing syscall handlers; `Vmar::protect` no-ops via `lookup_pa = None`; `mm::vmar::map` leaks PTEs on overflow; `KernelObject::duplicate` only supports Vmo; global `HANDLE_TABLE_LOCK` shared across per-process tables; 256-byte IPC cap; concurrent `sys_channel_lookup` collision; validation layer is a no-op facade; `alloc_page` is non-atomic; `static mut FUTEX_TABLE` race; per-process L0 init assumes AArch64; RISC-V L2 block shatter loses content; `ThreadContext` layout assert AArch64-only; port + futex syscalls unreachable |
| **Low** | ~30+ | `hal` crate entirely dead; `slab` allocator dead; `Mutex`/`Semaphore`/`Event` dead; `tick_count` / `tick()` unused; `AArch64Mmu::is_enabled` hard-coded `false`; `PathResolver`, `BootInfo::empty`, `MappingRequest`, `MessageMetadata` duplicated; many `VnodeType` / `ObjectType` / `Rights` variants never referenced; 7/10 `Rights` constants never used; `aarch64::console_putbytes`, `arch::console_getchar`, `arch::console::Console` dead; `BootInfo::empty` dead; `kcore::init` / `kcore::debug_print` dead; the entire `task::smoke.rs` is dead code; `mm::elf.rs::ElfLoader::is_valid` always returns true. |

---

## What would be needed for 1.0

1. **Re-enable RISC-V SV39 translation** and implement
   `translate_user_va` to walk the real page table (currently a
   stub).  Investigate the mstatus.MPP / `sfence.vma` ordering
   question flagged in the source comment.
2. **Fix the entry-VA relocation in
   `Process::launch_user_program_with_argv`** so multi-slot
   OHLINK binaries (loader at `0x90…`, devmgr at `0xA0…`, init
   at `0xB0…`) each land at their own VMAR slot rather than
   colliding at `0x9000_0000`.
3. **Wire up the missing ~18 syscall handlers** (or delete the
   syscall numbers) — channel_call, port_create/wait/queue,
   event_*, timer_*, futex_*, vmo_get_size, vmo_set_size,
   vmar_unmap, vmar_protect, thread_exit, process_start,
   process_exit, get_tid, get_pid.  Currently they all silently
   return `Status::NotAllowed`, which is an opaque failure for
   EL0 callers.
4. **Replace `sys_read` stub** with a real rootfs-backed read so
   fileagent and the shell can actually serve file contents.
5. **Decide on the `hal` crate** — either complete the
   `Cpu`/`Mmu`/`PageTable` impls and use them to replace the
   free-function MMU API, or delete the entire `kernel/hal` and
   `shared::boot` / `shared::ipc::Message` /
   `shared::MessageMetadata` / `shared::ChannelEndpoint` and
   the dead `arch::console` / `arch::cpu` /
   `arch::aarch64::console_putbytes` / `arch::console_getchar`
   facade items.
6. **Delete the dead `kcore::alloc` / `kcore::debug` modules** or
   wire them up.  `SimpleAllocator::alloc` returning `nullptr`
   is a latent landmine the day someone adds
   `extern crate alloc`.
7. **Delete or implement** `mm::slab`, `mm::elf` (or actually
   validate the OHLINK magic), `sync::primitives::{Mutex,
   Semaphore, Event}`, `object::handle::Handle`, `task::smoke`,
   `vfs::PathResolver`, `task::scheduler::{tick,
   current_thread_name}`.
8. **Fix `Vmar::protect` / `lookup_pa`** to actually return the
   backing PA, or remove the syscall.
9. **Make the per-process VMAR allocation collision-safe** — slot
   0 starts at `0x9000_0000` and the loader is at `0x90…`, so the
   loader's first EL0 PC and the kernel's MMIO at `0x0900_0000`
   may overlap on some configurations.
10. **Add real SMP / lock-IRQ-safe atomics** for
    `NEXT_FREE_PAGE`, `END_FREE_PAGE`, `FUTEX_TABLE`,
    `HANDLE_TABLE_LOCK`, `REGISTRY_LOCK` — all are `static mut`
    with non-atomic updates under a `compare_exchange_weak` lock
    that *enables* IRQs on release.
