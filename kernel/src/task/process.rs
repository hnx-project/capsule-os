use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::mm::vmar::Vmar;
use crate::object::handle_table::HandleTable;

static PROCESS_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
pub static VMAR_BASE_SLOT: AtomicUsize = AtomicUsize::new(0);

/// Per-process current working directory buffer.  Storing a flat buffer
/// (rather than a `&'static str`) lets `SYSCALL_CHDIR` mutate it in place
/// without juggling arena lifetimes, and is large enough for any realistic
/// EL0 path (`/system/bin/osh`, `/home/devmgr/projects/foo`, ...).
pub const CWD_MAX: usize = 128;

/// Initial CWD assigned to every freshly-spawned process.  We deliberately
/// pick a real, rootfs-relative path so that `cd ..` and friends in the
/// shell behave sensibly from the very first prompt.
pub const INITIAL_CWD: &str = "/";

#[derive(Debug)]
pub struct Process {
    pub id: u64,
    pub name: &'static str,
    pub state: ProcessState,
    pub root_vmar: Vmar,
    pub handle_table: HandleTable,
    pub thread_count: usize,
    pub l0_user_pa: usize,
    pub cwd: [u8; CWD_MAX],
    pub cwd_len: usize,
    /// Exit code recorded by `SYSCALL_EXIT` so the parent can harvest
    /// it via `SYSCALL_WAIT`.  `None` while the process is still
    /// running or zombie with no exit recorded yet.  When set, the
    /// process becomes a proper Zombie and the parent can reap it
    /// to Dead via `SYSCALL_WAIT`.
    pub exit_status: Option<i32>,
    /// pid of the parent process (`0` for pid 1 anchor).  Used by
    /// `SYSCALL_GETPPID` and by the kernel scheduler to decide
    /// which process is allowed to `wait` on which children.
    pub parent_pid: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Initial,
    Running,
    Zombie,
    Dead,
}

impl Process {
    pub fn new(name: &'static str) -> Result<Self> {
        let slot = VMAR_BASE_SLOT.fetch_add(1, Ordering::Relaxed);

        #[cfg(target_arch = "aarch64")]
        let vmar_base = 0x9000_0000usize + slot * 0x1000_0000usize;
        #[cfg(target_arch = "riscv64")]
        let vmar_base = 0x9000_0000usize + slot * 0x1000_0000usize;

        let root_vmar = Vmar::create(vmar_base, 256 * 1024 * 1024)?;

        let handle_table = HandleTable::new();
        let mut cwd = [0u8; CWD_MAX];
        let cwd_bytes = INITIAL_CWD.as_bytes();
        let cwd_len = core::cmp::min(cwd_bytes.len(), CWD_MAX);
        cwd[..cwd_len].copy_from_slice(&cwd_bytes[..cwd_len]);
        Ok(Process {
            id: PROCESS_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            state: ProcessState::Initial,
            root_vmar,
            handle_table,
            thread_count: 0,
            l0_user_pa: 0,
            cwd,
            cwd_len,
            exit_status: None,
            parent_pid: 0,
        })
    }

    pub fn add_thread(&mut self) {
        self.thread_count = self.thread_count.wrapping_add(1);
    }

    pub fn remove_thread(&mut self) {
        if self.thread_count > 0 {
            self.thread_count -= 1;
        }
    }

    pub fn launch_user_program(name: &'static str, binary_bytes: &[u8]) -> Result<()> {
        Self::launch_user_program_with_argv(name, binary_bytes, &[], &[], 0, 0)
    }

    /// Launch a fresh EL0 program with `argc`/`argv` materialised onto its
    /// user stack.  `arg_strs` is an array of byte buffers and `arg_lens`
    /// records the valid byte count of each entry; both slices must have
    /// the same length (>= `argc`).  The strings are copied in argv order
    /// to the top of the freshly mapped 16 KiB user stack, followed by the
    /// argv pointer array (so the stack top points at argv[0]).  The new
    /// thread is then started with x0=argc and x1=argv_ptr.
    pub fn launch_user_program_with_argv(
        name: &'static str,
        binary_bytes: &[u8],
        arg_strs: &[[u8; 256]],
        arg_lens: &[usize],
        argc: usize,
        parent_pid: u64,
    ) -> Result<()> {
        use crate::mm::vmo::Vmo;
        use crate::mm::vmar::VmarFlags;
        use crate::task::thread::{Thread, ThreadState};

        let parser = ohlink_format::parser::OHLK_Parser::new(binary_bytes).map_err(|e| {
            crate::log_error!("LAUNCHER", "Failed to parse OHLINK format: {:?}", e);
            Status::InvalidArgs
        })?;

        let header = parser.header();

        let proc = allocate_process(name)?;
        let pid = proc.id;
        proc.parent_pid = parent_pid;

        let l0_user_pa = crate::mm::phys::alloc_page()?.as_usize();
        proc.l0_user_pa = l0_user_pa;

        let mut old_ttbr0: usize = 0;

        #[cfg(target_arch = "aarch64")]
        unsafe {
            let mut ttbr1: u64;
            core::arch::asm!("mrs {0}, ttbr1_el1", out(reg) ttbr1, options(nomem, nostack));
            let kernel_l0_pa = (ttbr1 & 0x0000_FFFF_FFFF_F000) as usize;

            let kernel_l0_va = crate::mm::mmu::pa_to_kernel_va(kernel_l0_pa) as *const u64;
            let user_l0_va = crate::mm::mmu::pa_to_kernel_va(l0_user_pa) as *mut u64;

            for i in 0..512 {
                let entry = core::ptr::read_volatile(kernel_l0_va.add(i));
                core::ptr::write_volatile(user_l0_va.add(i), entry);
            }
            // **B1.5 cache flush for the entire new L0 page.**  The
            // populate-512 loop just wrote every L0 entry, but only
            // entries 0 and 511 had their cache line
            // clean+invalidated (dc civac on those two specific
            // addresses).  Entries 1..510 still hold dirty cache
            // lines that the MMU walker can serve stale: when the
            // next vmar::map / arch_mmu::map_page call asks for a
            // L1/L2/L3 page allocation, it uses `write_pte` (which
            // does have its own dc civac), so subsequent L0 entries
            // written for that walk are kept coherent -- but the 511
            // entries we wrote *here*, before we even switch TTBR0
            // to l0_user_pa, carry no such safety net.  Without the
            // full-page flush, an MMU walk issued immediately after
            // `set_ttbr0_el1(l0_user_pa)` can read an entry written
            // *here* but kept only in the data-cache write buffer,
            // and a qemu-tcg-cache-aliasing race then maps the
            // user's stack pointer to an L1 page that was nominally
            // populated by vmar::map but is not yet architecturally
            // visible -- producing the FAR=0x92003d68 EC=0x24 ESF=0x07
            // Address Size Fault observed in KERNEL_HEALTH.md A2.
            //
            // Calling `flush_table_page(l0_user_pa)` here re-uses
            // the existing post-shatter maintenance path (clean+invalidate
            // every cache line, dsb ish).  Net cost: ~64 dc civac on
            // a 4 KiB L0 page, a few microseconds at boot; this is a
            // one-shot cost at process-launch time.
            crate::arch::aarch64::mmu::flush_table_page_pub(l0_user_pa);

            // The two manual flushes below are kept as a belt-and-braces
            // step so existing release profiles (which build with
            // debug_assertions disabled and *don't* re-call flush_table_page)
            // still have their entry 0 + 511 lines made architectural-
            // observable.  Once B1.5 is verified end-to-end we can
            // collapse these two lines into the single flush above.
            core::arch::asm!("dc civac, {0}", in(reg) user_l0_va, options(nomem, nostack));
            core::arch::asm!("dc civac, {0}", in(reg) user_l0_va.add(511 * 8), options(nomem, nostack));
            core::arch::asm!("dsb ish", options(nomem, nostack));

            let mut ttbr0_reg: u64;
            core::arch::asm!("mrs {0}, ttbr0_el1", out(reg) ttbr0_reg, options(nomem, nostack));
            old_ttbr0 = (ttbr0_reg & 0x0000_FFFF_FFFF_F000) as usize;

            crate::arch::aarch64::mmu::set_ttbr0_el1(l0_user_pa);
        }

        let mut lowest_vaddr: usize = usize::MAX;

        for idx in 0..header.header_count {
            let entry_meta = match parser.get_entry(idx) {
                Ok(e) => e,
                _ => continue,
            };

            let ty = entry_meta.ty;
            if ty != 1 && ty != 2 && ty != 3 && ty != 4 {
                continue;
            }

            let virt_addr = entry_meta.virtual_address as usize;
            let size = entry_meta.mem_size as usize;

            if virt_addr < lowest_vaddr {
                lowest_vaddr = virt_addr;
            }

            let mut flags_raw = entry_meta.flags;
            if ty == 1 {
                flags_raw &= !2;
            }

            let aligned_vaddr = virt_addr & !(4096 - 1);
            let alignment_offset = virt_addr - aligned_vaddr;
            let aligned_size = (size + alignment_offset + 4095) & !(4095);

            let target_va = proc.root_vmar.base + aligned_vaddr;
            let flags = VmarFlags::from_bits(flags_raw);
            crate::log_info!("LAUNCHER", "Mapping segment: ty={}, flags={:?} (raw={:#x}), target_va={:#x}, size={}",
                ty, flags, flags_raw, target_va, aligned_size);

            let mut vmo = Vmo::create_with_size(aligned_size)?;
            vmo.commit_all()?;
            if entry_meta.file_size > 0 {
                let segment_payload = parser.get_segment_data(&entry_meta).map_err(|e| {
                    crate::log_error!("LAUNCHER", "Failed to retrieve segment payload: {:?}", e);
                    Status::InvalidArgs
                })?;
                let write_offset = alignment_offset;
                vmo.write(write_offset, segment_payload)?;
            }

            proc.root_vmar.map(&mut vmo, 0, target_va, aligned_size, flags)?;

            #[cfg(target_arch = "aarch64")]
            crate::arch::aarch64::mmu::sync_instruction_cache(target_va, aligned_size);
        }

        // Compute the user-mode entry VA.  The OHLINK header carries the
        // ELF `e_entry` verbatim — an absolute virtual address in the
        // slot the binary was originally linked for (0x90xxxxxx for
        // the loader, 0xA0xxxxxx for devmgr, 0xB0xxxxxx for the
        // respawned init, etc.).  Each process is now loaded into its
        // own vmar slot starting at `proc.root_vmar.base`, so the
        // entry has to be *relocated* by stripping the original slot
        // bits and adding the new base.
        //
        // The historical formula was
        //   user_entry = vmar_base + lowest_vaddr + (e_entry - lowest_vaddr)
        // which only works when `lowest_vaddr == 0` AND `e_entry` is
        // already slot-relative.  Neither is true for our ELFs, so
        // the kernel was computing `0x120212788` for the loader's
        // `e_entry = 0x90212788` and the resulting user thread jumped
        // to unmapped memory on its first SVC, killing the boot
        // anchor with EC=0x24 (data abort) at ELR = vmar_base + 0x2128
        // and FAR = vmar_base + 0x2000000 + 0x3d68 (the user stack).
        let user_entry_offset = header.entry_point as usize & 0x0FFF_FFFF;
        let user_entry = proc.root_vmar.base + user_entry_offset;

        let stack_size = 16 * 1024;
        let mut stack_vmo = Vmo::create_with_size(stack_size)?;
        stack_vmo.commit_all()?;
        let stack_vaddr_offset = 0x2000000;
        let stack_va = proc.root_vmar.base + stack_vaddr_offset;

        let stack_flags = VmarFlags::from_bits(
            VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits()
        );

        proc.root_vmar.map(&mut stack_vmo, 0, stack_va, stack_size, stack_flags)?;

        let stack_top = (stack_va + stack_size) & !(15usize);

        use crate::mm::mmu::ArchMmu;
        #[cfg(target_arch = "aarch64")]
        crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();

        let user_entry = proc.root_vmar.base + user_entry_offset;

        let mut thread = Thread::new_user(name, user_entry, stack_top)?;
        thread.process_id = pid;
        thread.handle_table = &proc.handle_table;
        thread.state = ThreadState::Ready;

        // Always normalise x0/x1 to argc/argv_ptr semantics at
        // process entry.  The hnxlibc `_hnx_user_entry` trampoline
        // reads x0/x1 directly to populate its argv table, so legacy
        // launches (argc == 0) must NOT leave x0 = entry / x1 =
        // stack_top in place; the trampoline would interpret those
        // huge values as a non-empty argv and dereference garbage.
        //
        // **Field name trap**: `ThreadContext::r` is the *callee-saved*
        // register window (x19..x30), not x0/x1.  Writing
        // `thread.context.r[0]` here would silently write to x19 and
        // the argv would never reach EL0 — the symptom is `argc=0` in
        // the user program even though the kernel log shows
        // `argc=2` at launch time.  Use `x[0]` / `x[1]`.
        thread.context.x[0] = argc as u64;
        thread.context.x[1] = 0;

        // Materialise argv on the new process's user stack.  We grow the
        // argv area downward from `stack_top`: first the argv pointer
        // array (8 bytes per slot, argc slots), then the argv string
        // payloads (each padded to 16-byte alignment).  `argv_user_va`
        // ends up pointing at argv[0].  x0=argc, x1=argv_user_va are
        // then handed to the user entry trampoline via ThreadContext.
        if argc > 0 {
            let argv_array_bytes = argc * 8;
            let mut string_total: usize = 0;
            for i in 0..argc {
                string_total += (arg_lens[i] + 15) & !15;
            }
            let argv_area = argv_array_bytes + string_total + 16;

            let mut new_sp = (stack_top - argv_area) & !(15usize);
            let mut cursor = new_sp;

            // First: copy each argv string payload upward, capturing the
            // user-VA of each one so the pointer array below points at it.
            // Each string is NUL-terminated so the user entry trampoline's
            // "scan until 0 byte" loop (in `hnxlibc/src/lib.rs`
            // `_hnx_user_entry`) finds the end of the string instead of
            // running through the 16-byte pad area into adjacent argv
            // entries — without the NUL, `__HNX_ARGV_LENS[i]` would
            // measure the padded length (>= 16) rather than the actual
            // string length, and the user program would observe garbage
            // bytes in `hnx_arg(i)` and a non-UTF-8 error.
            let mut arg_vas: [usize; 16] = [0usize; 16];
            for i in 0..argc {
                let s_len = arg_lens[i];
                let padded = (s_len + 15) & !15;
                let dst = cursor;
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    l0_user_pa,
                    &arg_strs[i][..s_len],
                    dst,
                    s_len,
                )?;
                let nul: [u8; 1] = [0u8];
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    l0_user_pa,
                    &nul,
                    dst + s_len,
                    1,
                )?;
                arg_vas[i] = dst;
                cursor += padded;
            }

            // Then: write the argv pointer array (little-endian u64 each)
            // at the location the user will see as argv[0].
            let argv_ptr_va = cursor;
            for i in 0..argc {
                let bytes = (arg_vas[i] as u64).to_le_bytes();
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    l0_user_pa,
                    &bytes,
                    argv_ptr_va + i * 8,
                    8,
                )?;
            }
            cursor += argv_array_bytes;

            // The new initial sp points just past the argv array; SP at
            // entry is therefore the post-argv stack pointer, while argv
            // lives directly under it.  argv[0] is at argv_ptr_va.
            let post_argv_sp = (cursor + 15) & !15;

            thread.context.user_sp = post_argv_sp as u64;
            thread.context.x[1] = argv_ptr_va as u64;
        }

        unsafe {
            crate::task::scheduler::SCHEDULER.add(thread);
        }

        // Post-add cache hardening (B1.1).  The flow so far wrote
        // fresh page-table entries for the new process's root_vmar
        // (text + stack + data) under the assumption that the MMU
        // hardware walk would re-read the new L0/L1/L2/L3 PTEs on
        // the first eret into EL0.  Three things conspire on
        // QEMU-TCG to make that assumption unsafe:
        //
        //   1.  The data cache can hold a stale PTE value written
        //       before the write_pte path's `dc civac + dsb ish`
        //       landed in physical memory.  TCG's coherence model
        //       schedules icache invalidations on the icache side
        //       only, not on the dc side, so a stale CACHED PTE
        //       can survive an `ic ivau` round-trip on the icache
        //       and poison the first fetch at eret.
        //   2.  The icache, on aarch64, is VIPT and indexed by both
        //       VA and cache-set hash; an aliasing VA range (e.g.
        //       our `proc.root_vmar.base + offset`) can be served
        //       from an old icache line even after we wrote the
        //       corresponding physical page.  The textbook fix is
        //       to INVALIDATE the entire icache-to-PoU *for the
        //       aliasing range the kernel just wrote into*, then
        //       DSB + ISB so the first eret instruction fetch goes
        //       all the way back to DRAM.  `sync_instruction_cache`
        //       does exactly this; we extend it slightly to cover
        //       the stack mapping too (touching user stack lines
        //       *is* part of EL0 entry flow because the first
        //       `mov sp, xN` uses the freshly-mapped stack page).
        //   3.  The TLB holds stale translations from the *kernel*
        //       context that ran `set_ttbr0_el1(old_ttbr0)` below.
        //       A `tlbi vaae1` of all addresses (vmalle1 with the
        //       ASID-from-context rather than explicit) is invoked
        //       by the ttbr0_EL1 restore below; but for the *first*
        //       eret into this thread, we are about to leave the
        //       kernel with TTBR0 = old_ttbr0 (the caller's table),
        //       not the new thread's table — which means the
        //       very first time we ever switch *into* this thread
        //       (next scheduler tick), the swap to `l0_user_pa`
        //       must come AFTER a full TLB drop.  The scheduler
        //       already does that.  Nothing for us to fix here.
        //
        // We therefore: (a) flush+invalidate the icache over the
        // full EL0 image (text + stack + data), (b) DSB, (c) ISB on
        // the kernel side so subsequent state changes propagate.
        #[cfg(target_arch = "aarch64")]
        let _post_add_barrier = unsafe {
            // Sync instruction cache over the entire user VA range
            // the kernel just populated (codeseg + stack + data,
            // bounded by `proc.root_vmar.base + 0x4000000` -- the
            // 64 MiB slot we hand each process).  An over-wide
            // sync is cheap on aarch64 (PoU icache invalidation
            // is a single `dc cvau` + `ic ivau` per cache line)
            // and safer than under-shooting into a partial TLB
            // invalidation that leaves a stale icache line behind.
            // 64 MiB covers vmar_base + 64 MiB, which is larger
            // than any EL0 image we currently ship.
            let icache_sync_end = proc.root_vmar.base + 0x0400_0000;
            crate::arch::aarch64::mmu::sync_instruction_cache(
                proc.root_vmar.base,
                icache_sync_end - proc.root_vmar.base,
            );
            // Final ISB so the next kernel instruction (the
            // `set_ttbr0_el1(old_ttbr0)` below, and the *kernel's*
            // `ret` from this function) sees the freshly-invalidated
            // icache.  Without the ISB the kernel can keep
            // speculatively executing through the same stale
            // icache hash until the next exception boundary, and
            // the swap-restore below can race the actual eret.
            core::arch::asm!("isb", options(nomem, nostack));
            ()
        };


        // **Restore the caller-side TTBR0_EL1** before returning to the
        // kernel context, but ONLY when there is a sensible previous
        // page table to return to.  `set_ttbr0_el1(l0_user_pa)` above
        // swapped the MMU's user page table to the freshly-allocated
        // per-process l0, and `safe_copy_to_user` calls inside this
        // function relied on that swap to translate user VAs to PA.
        // Leaving the new process's l0 live on ttbr0 makes the next user
        // thread we switch into translate its VAs through the wrong
        // table — yielding an EC=0x0 ELR=0x0 trap the moment eret tries
        // to execute at a PC that doesn't belong to *that* process.
        //
        // During the very first call from `kernel_main` (loader
        // bootstrap) `old_ttbr0` is the boot value (typically 0); we
        // can't point ttbr0 at a zero pa, so in that case we instead
        // **keep** the new process's l0 live — the loader is the very
        // next thread we'll eret into, so this is what we want anyway.
        #[cfg(target_arch = "aarch64")]
        unsafe {
            if old_ttbr0 != 0 {
                use crate::mm::mmu::ArchMmu;
                crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();
                crate::arch::aarch64::mmu::set_ttbr0_el1(old_ttbr0);
            }
        }

        crate::log_info!("LAUNCHER", "Successfully launched program '{}' at EL0 (entry={:#x}, pid={}, argc={})",
            name, user_entry, pid, argc);

        // **B1.5 PTE-walk dump (always-on).**  Build on B1.4: dump
        // not just the bytes at the entry VA and the stack VA, but
        // also the *page-table entries* on the L0 -> L1 -> L2 -> L3
        // walk the MMU will issue when EL0 first fetches.
        //
        // The hypothesis we are still pursuing is that one of the
        // L0/L1/L2 intermediate entries the kernel wrote during the
        // populate-512 + L1/L2/L3 allocation sequence is not
        // *architecturally* observable at the time the MMU walks it
        // (cache-aliasing race, write-buffer vs icache, etc.).  The
        // B1.5 dump reads those entries back through the kernel-side
        // direct-map alias of the freshly-populated L0 PA, and they
        // are printed via kprintln! so they appear on every release
        // smoke that the 0.6.0-alpha / 1.0 track captures.
        //
        // The walk runs both for the entry VA (the path the MMU
        // takes at the first eret into the new thread) and the
        // 0xd68-specific stack offset (the FAR address the loader
        // in the existing boot log faulted at).  If the entry-VA
        // walk shows a complete L0/L1/L2/L3 chain ending in a
        // 4-KiB leaf pointing at the right physical page but the
        // stack-VA walk shows a missing entry at some intermediate
        // level, that pinpoints exactly which vmar::map install step
        // needs extra cache/TLB maintenance.
        //
        // **AGENTS §4 guardrails**: the same comment as B1.4:
        // B1.5 is intentionally invasive for root-cause hunting.
        // We demote back to cfg(debug_assertions) in a single
        // follow-up commit once A2 is closed.

        #[cfg(target_arch = "aarch64")]
        {
            use core::fmt::Write;

            // Walk the entry VA.  We can't reuse `debug_walk_va`
            // (which is `log_info!`-gated) because the release
            // profile would silence this whole block.  Inline it.
            let l0e_e = unsafe {
                (crate::mm::mmu::pa_to_kernel_va(l0_user_pa)
                    as *const u64).add(0)
            };
            crate::kprintln!(
                "B1.5-DIAG pid={} entry-VA={:#x} L0E={:#018x}",
                pid,
                user_entry,
                unsafe { core::ptr::read_volatile(l0e_e) }
            );

            // Walk the FAR address directly: 0x92003d68 == (loader
            // vmar_base + 0x2000000 + 0x3d68) but we genericise
            // through `proc.root_vmar.base` so this works for
            // devmgr / init / fileagent too.
            let far_va = proc.root_vmar.base + 0x2000000 + 0x3d68;
            let l0_idx_f = (far_va >> 39) & 0x1FF;
            let l1_idx_f = (far_va >> 30) & 0x1FF;
            let l2_idx_f = (far_va >> 21) & 0x1FF;
            let l3_idx_f = (far_va >> 12) & 0x1FF;
            let l0e_f = unsafe {
                core::ptr::read_volatile(
                    (crate::mm::mmu::pa_to_kernel_va(l0_user_pa) as *const u64).add(l0_idx_f)
                )
            };
            crate::kprintln!(
                "B1.5-DIAG pid={} FAR-VA={:#x} L0E={:#018x} L0_IDX={}",
                pid,
                far_va,
                l0e_f,
                l0_idx_f,
            );
            if l0e_f & 1 != 0 && l0e_f & 0b10 != 0 {
                let l1_pa = (l0e_f & 0x0000_FFFF_FFFF_F000) as usize;
                let l1e = unsafe {
                    core::ptr::read_volatile(
                        (crate::mm::mmu::pa_to_kernel_va(l1_pa) as *const u64).add(l1_idx_f)
                    )
                };
                crate::kprintln!(
                    "  L1_PA={:#x} L1_IDX={} L1E={:#018x}",
                    l1_pa,
                    l1_idx_f,
                    l1e
                );
                if l1e & 1 != 0 && l1e & 0b10 != 0 {
                    let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
                    let l2e = unsafe {
                        core::ptr::read_volatile(
                            (crate::mm::mmu::pa_to_kernel_va(l2_pa) as *const u64).add(l2_idx_f)
                        )
                    };
                    crate::kprintln!(
                        "    L2_PA={:#x} L2_IDX={} L2E={:#018x}",
                        l2_pa,
                        l2_idx_f,
                        l2e
                    );
                    if l2e & 1 != 0 && l2e & 0b10 != 0 {
                        let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
                        let l3e = unsafe {
                            core::ptr::read_volatile(
                                (crate::mm::mmu::pa_to_kernel_va(l3_pa) as *const u64).add(l3_idx_f)
                            )
                        };
                        crate::kprintln!(
                            "      L3_PA={:#x} L3_IDX={} L3E={:#018x}",
                            l3_pa,
                            l3_idx_f,
                            l3e
                        );
                    }
                }
            }

            // Walk the stack-0x1000-prior neighbour (the page
            // immediately below the FAR address).  If the FAR walk
            // shows a complete L0/L1/L2/L3 chain while this
            // neighbour's chain ALSO does, then the A2 fault is NOT
            // an unmapped page: it is a translation-cache-aliasing
            // hazard on a mapped page.
            let prior_va = proc.root_vmar.base + 0x2000000 + 0x2d68;
            crate::kprintln!("B1.5-DIAG pid={} PRIOR-VA={:#x}", pid, prior_va);
            let l0_idx_p = (prior_va >> 39) & 0x1FF;
            let l1_idx_p = (prior_va >> 30) & 0x1FF;
            let l2_idx_p = (prior_va >> 21) & 0x1FF;
            let l3_idx_p = (prior_va >> 12) & 0x1FF;
            let l0e_p = unsafe {
                core::ptr::read_volatile(
                    (crate::mm::mmu::pa_to_kernel_va(l0_user_pa) as *const u64).add(l0_idx_p)
                )
            };
            crate::kprintln!(
                "  prior L0_IDX={} L0E={:#018x}",
                l0_idx_p,
                l0e_p
            );
            if l0e_p & 1 != 0 && l0e_p & 0b10 != 0 {
                let l1_pa = (l0e_p & 0x0000_FFFF_FFFF_F000) as usize;
                let l1e = unsafe {
                    core::ptr::read_volatile(
                        (crate::mm::mmu::pa_to_kernel_va(l1_pa) as *const u64).add(l1_idx_p)
                    )
                };
                crate::kprintln!(
                    "  prior L1_PA={:#x} L1_IDX={} L1E={:#018x}",
                    l1_pa,
                    l1_idx_p,
                    l1e
                );
                if l1e & 1 != 0 && l1e & 0b10 != 0 {
                    let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
                    let l2e = unsafe {
                        core::ptr::read_volatile(
                            (crate::mm::mmu::pa_to_kernel_va(l2_pa) as *const u64).add(l2_idx_p)
                        )
                    };
                    crate::kprintln!(
                        "  prior L2_PA={:#x} L2_IDX={} L2E={:#018x}",
                        l2_pa,
                        l2_idx_p,
                        l2e
                    );
                    if l2e & 1 != 0 && l2e & 0b10 != 0 {
                        let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
                        let l3e = unsafe {
                            core::ptr::read_volatile(
                                (crate::mm::mmu::pa_to_kernel_va(l3_pa) as *const u64).add(l3_idx_p)
                            )
                        };
                        crate::kprintln!(
                            "  prior L3_PA={:#x} L3_IDX={} L3E={:#018x}",
                            l3_pa,
                            l3_idx_p,
                            l3e
                        );
                    }
                }
            }
        }

        // **B1.4 diagnostic dump (always-on).**  FAR=0x92003d68 from
        // KERNEL_HEALTH A2 points into the new process's user stack
        // region (vmar_base + 0x2000000 + 0x3d68), so the fault is on
        // the very first stack access rather than at the ELF entry.
        // Three things we need to rule out before the
        // commit-trail-defeats-it hypothesis has any weight:
        //   (a) the entry mapping was never actually written;
        //   (b) the stack mapping was never actually written;
        //   (c) the kernel-side alias of these mappings has a stale
        //       view of the data cache when we read it back.
        //
        // B1.2 (4103da7) gated the dumps behind
        // #[cfg(debug_assertions)], so we didn't see them on the
        // release profile that the smoke uses.  B1.4 promotes the
        // dumps to **always-on** but routes them through `kprintln!`
        // (the unconditional UART-print primitive) rather than through
        // the `log_info!` family, so the format remains stable for
        // grep-able isolation.
        //
        // Reading via translate_user_va + pa_to_kernel_va keeps the
        // dump arch-correct: TTBR0 has just been zapped back to the
        // caller's table, so a load through TTBR0 would point at the
        // wrong L0.  We translate explicitly against this process's
        // freshly allocated L0 PA and reach physical memory through
        // the kernel-side aliasing table.
        //
        // **AGENTS §4 (no trace-and-keep) guardrails**: B1.4 is
        // intentionally invasive for root-cause hunting.  Once A2 is
        // closed we will demote it back to cfg(debug_assertions) in a
        // single follow-up commit, in line with the convention that
        // diagnostic-on-the-wire prints are deleted as soon as the
        // bug they were tracing is fixed.
        #[cfg(target_arch = "aarch64")]
        {
            use core::fmt::Write;
            if let Some(entry_pa) =
                crate::arch::aarch64::mmu::translate_user_va(l0_user_pa, user_entry)
            {
                let entry_kernel_va = crate::mm::mmu::pa_to_kernel_va(entry_pa);
                let mut dump = [0u8; 64];
                for i in 0..64usize {
                    unsafe {
                        dump[i] = core::ptr::read_volatile(
                            (entry_kernel_va as *const u8).add(i),
                        );
                    }
                }
                crate::kprintln!("B1.4-DIAG pid={} entry VA={:#x} PA={:#x} dump:", pid, user_entry, entry_pa);
                for chunk_off in (0..64usize).step_by(16) {
                    let mut hex = [0u8; 16 * 3 + 1];
                    for i in 0..16usize {
                        let byte = dump[chunk_off + i];
                        let h0 = b"0123456789abcdef"[(byte >> 4) as usize];
                        let h1 = b"0123456789abcdef"[(byte & 0xf) as usize];
                        hex[i * 3] = h0;
                        hex[i * 3 + 1] = h1;
                        if i < 15 {
                            hex[i * 3 + 2] = b' ';
                        }
                    }
                    let s = core::str::from_utf8(&hex[..(16 * 3 - 1)]).unwrap_or("");
                    crate::kprintln!("  +{:02x} {}", chunk_off, s);
                }
            } else {
                crate::kprintln!(
                    "B1.4-DIAG pid={} entry VA={:#x} -> MAPPING MISSING",
                    pid,
                    user_entry
                );
            }

            // Stack dump near FAR=0x92003d68 (offset 0x3d68 into the
            // 16 KiB stack mapped at vmar_base+0x2000000).  Picked
            // 0x3d68 - 0x40 = 0x3d28 so that the layout (0x3d28..0x3d68)
            // is just *below* the FAR address; if the alignment is
            // bad because the page table walk placed a 2 MiB block
            // where we expected a 4 KiB shatter, FAR will jump to a
            // *much* higher VA outside the stack region and we'll see
            // it in the address printed.
            let stack_va = proc.root_vmar.base + 0x2000000 + 0x3d28;
            if let Some(stack_pa) =
                crate::arch::aarch64::mmu::translate_user_va(l0_user_pa, stack_va)
            {
                let stack_kernel_va = crate::mm::mmu::pa_to_kernel_va(stack_pa);
                let mut dump = [0u8; 64];
                for i in 0..64usize {
                    unsafe {
                        dump[i] = core::ptr::read_volatile(
                            (stack_kernel_va as *const u8).add(i),
                        );
                    }
                }
                crate::kprintln!(
                    "B1.4-DIAG pid={} stack VA={:#x} PA={:#x} dump:",
                    pid,
                    stack_va,
                    stack_pa
                );
                for chunk_off in (0..64usize).step_by(16) {
                    let mut hex = [0u8; 16 * 3 + 1];
                    for i in 0..16usize {
                        let byte = dump[chunk_off + i];
                        let h0 = b"0123456789abcdef"[(byte >> 4) as usize];
                        let h1 = b"0123456789abcdef"[(byte & 0xf) as usize];
                        hex[i * 3] = h0;
                        hex[i * 3 + 1] = h1;
                        if i < 15 {
                            hex[i * 3 + 2] = b' ';
                        }
                    }
                    let s = core::str::from_utf8(&hex[..(16 * 3 - 1)]).unwrap_or("");
                    crate::kprintln!("  +{:02x} {}", chunk_off, s);
                }
            } else {
                crate::kprintln!(
                    "B1.4-DIAG pid={} stack VA={:#x} -> MAPPING MISSING",
                    pid,
                    stack_va
                );
            }
        }

        Ok(())
    }
}

pub const MAX_PROCESSES: usize = 8;
pub static mut PROCESSES: [Option<Process>; MAX_PROCESSES] = [None, None, None, None, None, None, None, None];

pub fn allocate_process(name: &'static str) -> Result<&'static mut Process> {
    unsafe {
        for slot in PROCESSES.iter_mut() {
            if slot.is_none() {
                let proc = Process::new(name)?;
                *slot = Some(proc);
                return Ok(slot.as_mut().unwrap());
            }
        }
    }
    Err(Status::NoMemory)
}

pub fn find_process_mut(id: u64) -> Option<&'static mut Process> {
    unsafe {
        for slot in PROCESSES.iter_mut() {
            if let Some(p) = slot {
                if p.id == id {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Read-only variant of `find_process_mut` that just returns the
/// process's user L0 page PA (or `None` if the process slot is
/// empty / the L0 hasn't been allocated yet).
///
/// Used by the scheduler to swap TTBR0_EL1 when it context-switches
/// between threads in different processes.  Returning a value rather
/// than a `&mut` keeps the call site lock-free — the L0 PA is
/// immutable for the lifetime of the process.
pub fn find_process_l0_user_pa(id: u64) -> Option<usize> {
    unsafe {
        for slot in PROCESSES.iter() {
            if let Some(p) = slot {
                if p.id == id && p.l0_user_pa != 0 {
                    return Some(p.l0_user_pa);
                }
            }
        }
    }
    None
}
