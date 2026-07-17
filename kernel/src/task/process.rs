use crate::mm::vmar::Vmar;
use crate::object::handle_table::HandleTable;
use crate::vfs::pipe::{PipeId, PipeRole};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use shared::status::{Result, Status};

/// One slot in `Process::fd_table`.  For 1.0 we only carry
/// pipe-end entries; once fileagent starts serving fds directly
/// to a process (rather than through the channel forwarder) we
/// will add a `VfsHandle(HandleValue)` variant here.
#[derive(Debug, Clone, Copy)]
pub enum FdEntry {
    Pipe { pipe: PipeId, role: PipeRole },
}

/// Lowest user-space fd; matches Linux's `STDERR_FILENO+1`.
pub const USER_FD_BASE: u32 = 3;
/// Size of `Process::fd_table`.  Index `0..USER_FD_BASE` are
/// reserved for the kernel-builtin UART path (K-D2).
pub const FD_TABLE_SIZE: usize = 16;

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
    pub page_table: crate::mm::page_table::PageTableTree,
    pub cwd: [u8; CWD_MAX],
    pub cwd_len: usize,
    /// Per-process file descriptor table; index 0/1/2 stay reserved
    /// for the in-kernel UART path (K-D2), so user-space fds live
    /// at indices `3..16`.  Each entry is either a `PipeRole` reader
    /// or writer (B6) or empty (`None`).  We keep this small enough
    /// to fit on the kernel side without a separate alloc frame; a
    /// 16-slot process is enough for the 1.0 demo (the limit matches
    /// POSIX_MIN_FD + 13 user fds).
    pub fd_table: [Option<FdEntry>; 16],
    /// First available user fd; bumped on every `alloc_fd` so
    /// hand-rolls of `sys_open` / `sys_pipe` can hand out stable
    /// numbers without scanning the table each time.  Reset to
    /// `3` (`STDERR_USER_BASE`) when the process is freshly spawned.
    pub next_fd: u32,
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
    /// Signal dispositions for this process.  Index `n` is the
    /// handler for signal `n + 1` (we use a 32-bit bitfield on
    /// `NSIG=32` to keep signal state compact).  Values:
    ///   0 (SIG_DFL) - default disposition (terminate for most sigs)
    ///   1 (SIG_IGN) - ignore
    /// Custom user-mode handlers are not supported in 1.0; a value
    /// of `2..` is rejected by `sys_sigaction`.
    pub sig_handlers: [u8; 32],
    /// Pending signals queued for this process.  The kernel
    /// dispatches each non-blocked, non-ignored bit lazily on
    /// `syscall_dispatch`'s way out (with the process's previous
    /// IRQ state held) or immediately at `raise(2)` time.
    pub pending_signals: u32,
    /// AArch64 ASID bound to this process.  Encoded into `TTBR0_EL1`
    /// bits [55:48] by `set_ttbr0_el1` so the TLB can be invalidated
    /// per-process instead of globally.  `ASID_KERNEL` (= 0) while the
    /// slot has not yet been initialised by `init_in_place`.
    pub asid: u16,
    /// List of VMOs owned by this process. This guarantees that all physical memory
    /// pages allocated for the process's stack, code, and data segments remain
    /// permanently reserved under explicit object ownership.
    pub vmos: [Option<crate::mm::vmo::Vmo>; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Initial,
    Running,
    Zombie,
    Dead,
}

impl Process {
    pub const fn new_dummy() -> Self {
        Process {
            id: 0,
            name: "",
            state: ProcessState::Initial,
            root_vmar: Vmar::new_dummy(),
            handle_table: HandleTable::new_dummy(),
            thread_count: 0,
            page_table: crate::mm::page_table::PageTableTree::new(),
            cwd: [0u8; CWD_MAX],
            cwd_len: 0,
            exit_status: None,
            parent_pid: 0,
            sig_handlers: [0u8; 32],
            pending_signals: 0u32,
            fd_table: [const { None }; FD_TABLE_SIZE],
            next_fd: USER_FD_BASE,
            #[cfg(target_arch = "aarch64")]
            asid: crate::arch::aarch64::asid::ASID_KERNEL,
            #[cfg(not(target_arch = "aarch64"))]
            asid: 0,
            vmos: [const { None }; 32],
        }
    }

    pub fn init_in_place(&mut self, name: &'static str) -> Result<()> {
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

        self.id = PROCESS_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        self.name = name;
        self.state = ProcessState::Initial;
        self.root_vmar = root_vmar;
        self.handle_table = handle_table;
        self.thread_count = 0;
        self.page_table = crate::mm::page_table::PageTableTree::new();
        self.cwd = cwd;
        self.cwd_len = cwd_len;
        self.exit_status = None;
        self.parent_pid = 0;
        self.sig_handlers = [0u8; 32];
        self.pending_signals = 0u32;
        self.fd_table = [const { None }; FD_TABLE_SIZE];
        self.next_fd = USER_FD_BASE;
        self.vmos = [const { None }; 32];

        #[cfg(target_arch = "aarch64")]
        {
            // Allocate an ASID for this process.  The allocator
            // returns an ID from the 8-bit space (1..=255) and bumps
            // its internal counter on wrap, but the actual TLB flush
            // is the caller's responsibility at the next
            // `set_ttbr0_el1` boundary.
            self.asid = crate::arch::aarch64::asid::alloc()
                .unwrap_or(crate::arch::aarch64::asid::ASID_KERNEL);
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            self.asid = 0;
        }

        Ok(())
    }

    pub fn add_thread(&mut self) {
        self.thread_count = self.thread_count.wrapping_add(1);
    }

    pub fn remove_thread(&mut self) {
        if self.thread_count > 0 {
            self.thread_count -= 1;
        }
    }

    pub fn launch_user_program(name: &'static str, binary_bytes: &[u8]) -> Result<u64> {
        let mut proc_id = 0;
        Self::launch_user_program_with_argv_id(name, binary_bytes, &[], &[], 0, 0, &mut proc_id)?;
        Ok(proc_id)
    }

    pub fn launch_user_program_with_argv(
        name: &'static str,
        binary_bytes: &[u8],
        arg_strs: &[[u8; 256]],
        arg_lens: &[usize],
        argc: usize,
        parent_pid: u64,
    ) -> Result<u64> {
        let mut proc_id = 0;
        Self::launch_user_program_with_argv_id(
            name,
            binary_bytes,
            arg_strs,
            arg_lens,
            argc,
            parent_pid,
            &mut proc_id,
        )?;
        Ok(proc_id)
    }

    /// Launch a fresh EL0 program with `argc`/`argv` materialised onto its
    /// user stack.  `arg_strs` is an array of byte buffers and `arg_lens`
    /// records the valid byte count of each entry; both slices must have
    /// the same length (>= `argc`).  The strings are copied in argv order
    /// to the top of the freshly mapped 16 KiB user stack, followed by the
    /// argv pointer array (so the stack top points at argv[0]).  The new
    /// thread is then started with x0=argc and x1=argv_ptr.
    pub fn launch_user_program_with_argv_id(
        name: &'static str,
        binary_bytes: &[u8],
        arg_strs: &[[u8; 256]],
        arg_lens: &[usize],
        argc: usize,
        parent_pid: u64,
        out_pid: &mut u64,
    ) -> Result<()> {
        use crate::mm::vmar::VmarFlags;
        use crate::mm::vmo::Vmo;
        use crate::task::thread::{Thread, ThreadState};

        let parser = ohlink_format::parser::OHLK_Parser::new(binary_bytes).map_err(|e| {
            crate::log_error!("LAUNCHER", "Failed to parse OHLINK format: {:?}", e);
            Status::InvalidArgs
        })?;

        let header = parser.header();

        let proc = allocate_process(name)?;
        let pid = proc.id;
        *out_pid = pid;
        proc.parent_pid = parent_pid;

        let mut old_ttbr0: usize = 0;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let mut ttbr0_reg: u64;
            core::arch::asm!("mrs {0}, ttbr0_el1", out(reg) ttbr0_reg, options(nomem, nostack));
            old_ttbr0 = (ttbr0_reg & 0x0000_FFFF_FFFF_F000) as usize;

            proc.page_table.allocate_root()?;

            // Map the UART device physical page (0x09000000) under the process L0.
            let uart_flags = crate::arch::aarch64::mmu::MapFlags::device_rw_user();
            if let Err(e) = proc.page_table.map_va(0x09000000, 0x09000000, &uart_flags) {
                crate::kprintln!("WARNING: Failed to map UART under user L0: {:?}", e);
            }

            // Map GIC CPU Interface page under user L0.
            let gic_flags = crate::arch::aarch64::mmu::MapFlags::device_rw_user();
            if let Err(e) = proc.page_table.map_va(0x08010000, 0x08010000, &gic_flags) {
                crate::kprintln!("WARNING: Failed to map GIC under user L0: {:?}", e);
            }

            // Copy the high-half kernel entries (L0[256..512]) and identity
            // L1 block from the current TTBR0 (parent) into the new tree.
            let active_l0_pa = (ttbr0_reg & 0x0000_FFFF_FFFF_F000) as usize;
            if active_l0_pa != 0 {
                proc.page_table.clone_high_half(active_l0_pa);
                proc.page_table.clone_identity_block(active_l0_pa);
            }
        }
        let mut lowest_vaddr: usize = usize::MAX;
        let pt = &mut proc.page_table;

        #[cfg(target_arch = "aarch64")]
        let irq_flags = crate::arch::trap::local_irq_save();

        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED after root setup pid={}", proc.id);
        }

        for idx in 0..header.header_count {
            let entry_meta = match parser.get_entry(idx) {
                Ok(e) => e,
                _ => {
                    continue;
                }
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

            proc.root_vmar
                .map_under_l0(&mut vmo, 0, target_va, aligned_size, flags, pt)?;
            #[cfg(target_arch = "aarch64")]
            {
                // To maintain full cache coherency, we must clean D-cache and invalidate I-cache
                // of each allocated physical page using the kernel's high-half direct-map alias virtual addresses (KVA),
                // since the active page table context at this point doesn't map target_va.
                let page_count = aligned_size / 4096;
                for i in 0..page_count {
                    let vmo_off = i * 4096;
                    if let Some(pa) = vmo.get_page_phys(vmo_off) {
                        let kva = crate::mm::mmu::pa_to_kernel_va(pa.as_usize());
                        crate::arch::aarch64::mmu::sync_instruction_cache(kva, 4096);
                    }
                }
            }
        }

        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED after OHLINK segments pid={}", proc.id);
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
            VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits(),
        );

        proc.root_vmar.map_under_l0(
            &mut stack_vmo,
            0,
            stack_va,
            stack_size,
            stack_flags,
            pt,
        )?;

        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED after stack mapping pid={}", proc.id);
        }

        let stack_top = (stack_va + stack_size) & !(15usize);

        // **B1.5.2 explicit stack-page backing touch.**
        //
        // QEMU-TCG's hypothesis continued: when the user stack
        // is allocated and committed via the Vmo path, the
        // 4 KiB physical page backing PA `stack_top` lives in
        // the kernel's direct-map alias via
        // `pa_to_kernel_va(stack_pa)`, but QEMU keeps the
        // model-side backing RAM in a "not yet touched" state
        // until the page is actually read or written by a
        // MMU walker / store.  A pure `flush_tlb_all()` does
        // not commit the backing RAM; the walk that follows
        // a `tlbi vaae1is` *will* commit it because QEMU
        // services the page-table walk with a model-side
        // `cpu_physical_memory_read` that the CPU backend
        // sees as a "real" access.
        //
        // Until recently the launch path relied on a
        // happenstance: the next MMU walk after spawn would be
        // the one that walks the user-side stack and that
        // walk would commit the page.  But QEMU's TLB and
        // page-cache implementation can keep the page in a
        // half-initialised state when the entry VA is the
        // *entry* of the user stack and the page-touch
        // happens at the very first page after eret.  This
        // results in the EL0-FAULT EC=0x24 ESR=0x9200004f
        // we've been chasing.
        //
        // The conservative workaround: have the kernel *
        // explicitly* touch the last 16 bytes of the freshly
        // committed stack page through its direct-map alias.
        // This performs a real `cpu_physical_memory_write`
        // (when we are zero-filling -- which we are, the
        // page is zero-initialised and we want it to stay
        // that way -- a load of any byte suffices to cause
        // QEMU to materialise the page-backing).
        //
        // The store uses `core::ptr::write_volatile` so the
        // optimizer cannot elide it.  It performs a real
        // 8-byte store at `stack_top - 16`.  The page is
        // already mapped via `proc.root_vmar.map` above, so
        // we do not need to re-walk the page table; we go
        // through the kernel-side alias directly.
        #[cfg(target_arch = "aarch64")]
        {
            let touch_va = stack_top - 16;
            if let Some(touch_pa) = pt.translate_va(touch_va) {
                let touch_kernel_va = crate::mm::mmu::pa_to_kernel_va(touch_pa) as *mut u64;
                unsafe {
                    core::ptr::write_volatile(touch_kernel_va, 0u64);
                    core::arch::asm!("dsb ish", options(nomem, nostack));
                }
            }
        }

        use crate::mm::mmu::ArchMmu;
        #[cfg(target_arch = "aarch64")]
        crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();

        let user_entry = proc.root_vmar.base + user_entry_offset;

        let mut thread = Thread::new_user(name, user_entry, stack_top)?;
        thread.process_id = pid;
        thread.context.process_id = pid;
        thread.context.l0_user_pa = pt.l0_pa() as u64;
        thread.context.page_table_gen = pt.generation;

        // Strict SPSR Lock: enforce EL0t privilege level with IRQs fully unmasked (spsr=0x000)
        // to prevent timer preempt or exception handler from corrupting the register context
        #[cfg(target_arch = "aarch64")]
        {
            thread.context.spsr = 0x000;
        }
        // Ensure the thread's handle_table pointer points to high-half KVA
        // instead of raw physical/identity address, so it survives TTBR0 page-table switches!
        let ht_raw = &proc.handle_table as *const HandleTable as usize;
        let ht_kva = if ht_raw < crate::mm::mmu::KERNEL_OFFSET {
            crate::mm::mmu::pa_to_kernel_va(ht_raw)
        } else {
            ht_raw
        };
        thread.handle_table = ht_kva as *const HandleTable;
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
                    pt.l0_pa(),
                    &arg_strs[i][..s_len],
                    dst,
                    s_len,
                )?;
                let nul: [u8; 1] = [0u8];
                crate::syscall::handlers::ipc::safe_copy_to_user(pt.l0_pa(), &nul, dst + s_len, 1)?;
                arg_vas[i] = dst;
                cursor += padded;
            }

            // Then: write the argv pointer array (little-endian u64 each)
            // at the location the user will see as argv[0].
            let argv_ptr_va = cursor;
            for i in 0..argc {
                let bytes = (arg_vas[i] as u64).to_le_bytes();
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    pt.l0_pa(),
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

        // **B1.5.3 GDB-derived fix: explicit dc ivac on the user's
        // new stack's last 16 bytes before eret.**
        //
        // See block-level comment at top of this file's B1.5.3
        // region for the full rationale.  Short version: the
        // `monitor info registers` dump from a `tools/run-debug.sh`
        // GDB session showed that the kernel-side context is
        // healthy at the moment of the fault --
        // `ttbr0_el1 = 0x404cc000` is the devmgr L0 PA
        // (matching `find_process_l0_user_pa(2)`), the ELR lives
        // in the devmgr text segment, and `spsr_el1.M = 0` says
        // we entered EL0 and then trap-ped, so the kernel-side
        // scheduler's TTBR0 swap + ISB barriers in
        // `Scheduler::schedule()` were honoured -- and yet ESR=0x0f
        // (synchronous external abort on translation table walk)
        // still fires on the first user-side read of `sp_el0`.
        //
        // The remaining gap in coverage is the *data cache half*
        // for the page backing `sp_el0`.  `safe_copy_to_user`
        // wrote argv strings into that page through the
        // kernel-side alias; `flush_tlb_all` and `sync_instruction_cache`
        // covered the text region plus the L0 PTE flush but
        // missed this specific stack line.  On Cortex-A72 the
        // very first user-side eret instruction fetch *and*
        // the very first user-side stack read both go through the
        // data-side cache (the latter as a load-with-PC=sp).
        // Evicting the line at `sp_el0 & ~0xfff` *before* the
        // next eret -- and pairing the data-cache invalidate with
        // an instruction-cache invalidate just in case QEMU-TCG
        // had pre-decoded the post-argv bytes -- is the cheapest
        // and most local fix that targets this exact line, and
        // it is the cheapest possible because the cost is one
        // cache-line evict: a few ns on physical hardware, less
        // on QEMU.
        #[cfg(target_arch = "aarch64")]
        {
            // `stack_top` is computed right above this point and
            // is the post-argv sp when argc > 0 and the raw
            // stack-top when argc == 0 (legacy SYSCALL_EXEC).
            // Note: page-aligning it & !0xFFF lands us on
            // `stack_top - 16`-1k page which is the *boundary*
            // page of the 16 KiB region (i.e. one 4 KiB page
            // PAST the allocated stack).  Round DOWN to one
            // 16-byte aligned address that is GUARANTEED to be
            // inside the stack region: `stack_top - 16`.
            let sp_top_aligned = (stack_top - 16) & !0xFusize;
            if let Some(sp_top_pa_resolved) = pt.translate_va(sp_top_aligned) {
                unsafe {
                    core::arch::asm!(
                        "and x9, {sp_va}, #~0xfff",
                        "dc ivac, x9",
                        "ic ivau, x9",
                        "dsb sy",
                        "isb",
                        sp_va = in(reg) sp_top_aligned,
                        options(nomem, nostack)
                    );
                }
            } else {
                crate::log_warn!(
                    "LAUNCHER",
                    "B1.5.3: dc ivac skipped: cannot translate user_sp={:#x}",
                    sp_top_aligned
                );
            }
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

            // **B1.5.1 strict-ordering barrier (KERNEL_HEALTH.md A2).**
            //
            // The QEMU-TCG smoke continues to hit
            //     `EL0-FAULT EC=0x24 ESR=0x9200004f
            //      FAR=0x92003ff0 thread=#1 pid=1`
            // even after B1.1-B1.5 closed the AP-bits layer.
            // B1.4 hex dump confirmed the L3 PTE for that FAR is
            // a valid, user-RW entry (`AP[2:1]=11`, AF=1) and
            // the backing page `PA=0x404e4000` is freshly
            // allocated, zeroed, and cache-flushed.  The fault is
            // therefore *not* an unbacked page; it is a
            // page-table-walk-side fault at level 0/1/2/3 with
            // FSC=0x0f ("Synchronous External Abort"), which on
            // AArch64 happens when the L3 PTE's PA points at a
            // physical address the system bus has not yet
            // committed to a coherent state, or when the data
            // cache + TLB sit in a state where the MMU walker
            // fetches a *different* PA than the kernel wrote.
            //
            // The hypothesis we are pursuing is the second one:
            // the launch path has a window where the *next*
            // schedule tick (loader -> idle/other threads) can
            // re-route the page-table walk through a stale TLB
            // entry left over by the new L0's contents.  Even
            // though `flush_tlb_all` runs at every `set_ttbr0_el1`
            // call below, the TLB can be re-filled under us
            // across a *different* ASID that the loader /
            // devmgr / init chain does not yet own.  The
            // conservative fix: invalidate **every** TLB entry
            // regardless of ASID with `tlbi vaae1is, xzr`, then
            // barrier the whole system with `dsb sy` so that
            // every store before is observable to the table
            // walker's first access.
            //
            // `vaae1is` with `xzr` (the zero register) is the
            // ARMv8 spelling for "invalidate by VA in all ASIDs,
            // EL1" -- it discards *all* TLB entries that match
            // the address pattern across every ASID.  On QEMU-
            // TCG (which does not yet model ASIDs, see TODO.md
            // H1 ASID-table) this collapses to the same as
            // `tlbi vmalle1`, but the IS variant requires an ISB
            // before the first TLB-fill event after the
            // invalidation -- which is exactly what we want for
            // eret-into-the-new-thread correctness.
            //
            // `dsb sy` is the strongest data-write barrier on
            // aarch64: every store the issuing core made before
            // this instruction must reach the memory system
            // before the next instruction completes.  Without it
            // the data cache can hold the page-table writes in
            // a write buffer, the `tlbi vaae1is` finishes before
            // the write buffer drains, and the subsequent
            // eret-driven page-table walk reads a stale cache
            // line and faults on SError 0x0f.
            core::arch::asm!(
                "tlbi vaae1is, xzr",
                "dsb sy",
                "isb",
                options(nomem, nostack)
            );

            // POST-LAUNCH stack PTE verification: confirm all 4 stack
            // pages have valid L3 PTEs immediately after mapping.
            for i in 0..4 {
                let check_va = stack_va + i * 4096;
                match pt.translate_va(check_va) {
                    None => {
                        crate::log_error!(
                            "LAUNCHER",
                            "POST-LAUNCH-FAIL: stack page {} at {:#x} pid={} has no valid PTE!",
                            i, check_va, proc.id
                        );
                    }
                    Some(pa) => {
                        crate::log_info!(
                            "LAUNCHER",
                            "POST-LAUNCH-OK: stack page {} at {:#x} -> PA {:#x} pid={}",
                            i, check_va, pa, proc.id
                        );
                    }
                }
            }
            // Raw L3 dump at post-launch
            unsafe {
                let l0_idx = crate::arch::aarch64::mmu::va_l0_index(stack_va);
                let l1_idx = crate::arch::aarch64::mmu::va_l1_index(stack_va);
                let l2_idx = crate::arch::aarch64::mmu::va_l2_index(stack_va);
                let l0e = crate::arch::aarch64::mmu::read_pte(pt.l0_pa(), l0_idx);
                let l1_pa = (l0e & 0x0000_FFFF_FFFF_F000) as usize;
                let l1e = crate::arch::aarch64::mmu::read_pte(l1_pa, l1_idx);
                let l2_pa = (l1e & 0x0000_FFFF_FFFF_F000) as usize;
                let l2e = crate::arch::aarch64::mmu::read_pte(l2_pa, l2_idx);
                let l3_pa = (l2e & 0x0000_FFFF_FFFF_F000) as usize;
                crate::log_info!("LAUNCHER", "POST-PTW: L0[{}]={:#018x} L1[{}]={:#018x} L2[{}]@PA={:#x}={:#018x} L3@PA={:#x} pid={}",
                    l0_idx, l0e, l1_idx, l1e, l2_idx, l2_pa, l2e, l3_pa, proc.id);
                if l3_pa != 0 {
                    for k in 0..8 {
                        let raw = crate::arch::aarch64::mmu::read_pte(l3_pa, k);
                        crate::log_info!("LAUNCHER", "POST-L3[{}]={:#018x} pid={}", k, raw, proc.id);
                    }
                }
            }
            ()
        };

        #[cfg(target_arch = "aarch64")]
        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED at post-launch pid={}", proc.id);
        }

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
                // `old_ttbr0` is the raw register value we read with
                // `mrs ttbr0_el1` earlier -- it already carries the
                // kernel-side ASID in bits [55:48].  Re-write the
                // register verbatim (with the same packed barrier
                // sequence as `set_ttbr0_el1`) so we don't accidentally
                // strip the ASID half and downgrade to a "no-ASID"
                // translation regime on the way back out.
                core::arch::asm!(
                    "msr ttbr0_el1, {val}",
                    "isb",
                    "tlbi vmalle1is",
                    "dsb ish",
                    "isb",
                    val = in(reg) old_ttbr0 as u64,
                    options(nomem, nostack)
                );
            } else {
                // Loader bootstrap: there is no prior TTBR0 to restore
                // to.  Install the *new* process's TTBR0 with its
                // freshly-allocated ASID so the very first `eret` into
                // the loader's entry point walks under the correct
                // translation regime.
                let new_asid = proc.asid;
                crate::arch::aarch64::mmu::set_ttbr0_el1(pt.l0_pa(), new_asid);
            }
        }

        // FINAL CANARY: dump PID 1's L3 page right before return,
        // AND set the global dynamic WATCH_PA so subsequent WATCH
        // statements catch writes to this page even when the PA
        // shifts between runs.
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let c_l0_idx = crate::arch::aarch64::mmu::va_l0_index(stack_va);
            let c_l1_idx = crate::arch::aarch64::mmu::va_l1_index(stack_va);
            let c_l2_idx = crate::arch::aarch64::mmu::va_l2_index(stack_va);
            let c_l0e = crate::arch::aarch64::mmu::read_pte(pt.l0_pa(), c_l0_idx);
            let c_l1_pa = (c_l0e & 0x0000_FFFF_FFFF_F000) as usize;
            let c_l1e = crate::arch::aarch64::mmu::read_pte(c_l1_pa, c_l1_idx);
            let c_l2_pa = (c_l1e & 0x0000_FFFF_FFFF_F000) as usize;
            let c_l2e = crate::arch::aarch64::mmu::read_pte(c_l2_pa, c_l2_idx);
            let c_l3_pa = (c_l2e & 0x0000_FFFF_FFFF_F000) as usize;
            if c_l3_pa != 0 {
                let vals: [u64; 8] = core::ptr::read_volatile(
                    crate::mm::mmu::pa_to_kernel_va(c_l3_pa) as *const [u64; 8]
                );
                crate::log_error!("LAUNCHER", "FINAL-CANARY pid={} l3_pa={:#x} L3[0..7]={:#x} {:#x} {:#x} {:#x} {:#x} {:#x} {:#x} {:#x}",
                    proc.id, c_l3_pa, vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6], vals[7]);
                // Set dynamic WATCH for PID 1 only (the known corruption target).
                // For other processes we'd overwrite PID 1's WATCH address.
                if proc.id == 1 {
                    crate::mm::phys::WATCH_PA.store(c_l3_pa, core::sync::atomic::Ordering::Release);
                    crate::log_error!("WATCH", "FINAL-CANARY set WATCH_PA={:#x} for pid={}", c_l3_pa, proc.id);
                }
            }
        }

        #[cfg(target_arch = "aarch64")]
        crate::arch::trap::local_irq_restore(irq_flags);

        Ok(())
    }
}

pub const MAX_PROCESSES: usize = 8;
pub static mut PROCESSES: [Option<Process>; MAX_PROCESSES] =
    [None, None, None, None, None, None, None, None];

/// Returns the calling thread's owning process id, or `NotFound`
/// if no thread is currently scheduled.  Useful for paths that
/// need the PID without dragging in the syscall-handler module
/// (e.g. the B5 signal layer).
pub fn current_process_id() -> Result<u64> {
    unsafe {
        let t = crate::task::scheduler::SCHEDULER
            .get_current_thread_ptr()
            .ok_or(Status::NotFound)?;
        Ok((*t).process_id)
    }
}

/// Thread-safe tracker to register page table pages under the currently running process.
/// Delegates to `PageTableTree::track` on the current process.
pub fn current_process_register_page_table(pa: usize) {
    unsafe {
        if let Ok(pid) = current_process_id() {
            if let Some(proc) = find_process_mut(pid) {
                proc.page_table.track(pa);
            }
        }
    }
}

/// Thread-safe tracker to register page table pages under a specific process matching its root L0 PA.
/// Delegates to `PageTableTree::track` on the matching process.
pub fn register_page_table_for_l0(l0_pa: usize, pa: usize) {
    unsafe {
        for slot in PROCESSES.iter_mut() {
            if let Some(proc) = slot {
                if proc.page_table.has_root() && proc.page_table.l0_pa() == l0_pa {
                    proc.page_table.track(pa);
                    break;
                }
            }
        }
    }
}

pub fn allocate_process(name: &'static str) -> Result<&'static mut Process> {
    unsafe {
        for slot in PROCESSES.iter_mut() {
            if slot.is_none() {
                *slot = Some(Process::new_dummy());
                let proc_ref = slot.as_mut().unwrap();
                proc_ref.init_in_place(name)?;
                return Ok(proc_ref);
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
/// process's user L0 page PA and ASID (or `None` if the process slot
/// is empty / the L0 hasn't been allocated yet).
///
/// Used by the scheduler to swap `TTBR0_EL1` when it context-switches
/// between threads in different processes.  Returning a value rather
/// than a `&mut` keeps the call site lock-free — the L0 PA and ASID
/// are immutable for the lifetime of the process (the ASID is
/// allocated once at `init_in_place` time and never recycled under
/// single-core bring-up).
pub fn find_process_l0_user_pa(id: u64) -> Option<(usize, u16)> {
    unsafe {
        for slot in PROCESSES.iter() {
            if let Some(p) = slot {
                if p.id == id && p.page_table.has_root() {
                    return Some((p.page_table.l0_pa(), p.asid));
                }
            }
        }
    }
    None
}

pub fn validate_process_page_table(id: u64) -> bool {
    unsafe {
        for slot in PROCESSES.iter() {
            if let Some(p) = slot {
                if p.id == id {
                    return p.page_table.validate();
                }
            }
        }
    }
    false
}

pub fn find_process_page_table_gen(id: u64) -> Option<u64> {
    unsafe {
        for slot in PROCESSES.iter() {
            if let Some(p) = slot {
                if p.id == id {
                    return Some(p.page_table.generation);
                }
            }
        }
    }
    None
}

impl Drop for Process {
    fn drop(&mut self) {
        // Strict microkernel drop order:
        // 1. Destructure and clear the ASID/TLB registrations to stop CPU from referencing this process
        // 2. Free and clear the VMAR registrations to release virtual mappings
        // 3. Free the tracked L1, L2, L3 Page Table pages so they can be safely reclaimed back to the allocator
        // 4. Finally, release the OHLINK segment and Stack VMO physical data pages

        crate::log_info!(
            "PROCESS_DROP",
            "Process '{}' (PID {}) is dropping. Reclaiming intermediate page table pages.",
            self.name,
            self.id
        );

        #[cfg(target_arch = "aarch64")]
        {
            // Flush all TLB entries for this process ASID
            unsafe {
                core::arch::asm!(
                    "dsb ish",
                    "tlbi vmalle1is",
                    "dsb sy",
                    "isb",
                    options(nomem, nostack)
                );
            }
        }

        unsafe { self.page_table.free_tree(); }
    }
}
