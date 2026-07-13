use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::mm::vmar::Vmar;
use crate::object::handle_table::HandleTable;
use crate::vfs::pipe::{PipeId, PipeRole};

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
    pub l0_user_pa: usize,
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
            sig_handlers: [0u8; 32],
            pending_signals: 0u32,
            fd_table: [const { None }; FD_TABLE_SIZE],
            next_fd: USER_FD_BASE,
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

        crate::kprintln!("[DIAG] launch_user_program ENTRY, binary_bytes.len={}", binary_bytes.len());

        let parser = ohlink_format::parser::OHLK_Parser::new(binary_bytes).map_err(|e| {
            crate::log_error!("LAUNCHER", "Failed to parse OHLINK format: {:?}", e);
            Status::InvalidArgs
        })?;

        let header = parser.header();

        let proc = allocate_process(name)?;
        let pid = proc.id;
        proc.parent_pid = parent_pid;

        let l0_user_pa = crate::mm::phys::alloc_page()?.as_usize();
        let user_l1_pa = crate::mm::phys::alloc_page()?.as_usize();
        proc.l0_user_pa = l0_user_pa;

        let mut old_ttbr0: usize = 0;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            crate::arch::aarch64::mmu::zero_page(l0_user_pa);
            crate::arch::aarch64::mmu::flush_table_page_pub(l0_user_pa);
            crate::arch::aarch64::mmu::zero_page(user_l1_pa);
            crate::arch::aarch64::mmu::flush_table_page_pub(user_l1_pa);

            let l0_kva = crate::mm::mmu::pa_to_kernel_va(l0_user_pa) as *mut u64;
            let l1_entry = ((user_l1_pa as u64) & 0x0000_FFFF_FFFF_F000)
                | 1
                | 3;
            core::ptr::write_volatile(l0_kva, l1_entry);
            core::arch::asm!("dc cvac, {0}", in(reg) l0_kva as usize, options(nomem, nostack));
            core::arch::asm!("dsb ish", options(nomem, nostack));
            crate::kprintln!("[DIAG] user L0[0] -> user L1={:#x}", user_l1_pa);

            // Crucial Fix: Map the UART device physical page (0x09000000) under the process L0 page directory.
            // When we run in userspace with TTBR0_EL1, any kernel trap/SVC print statement uses 0x09000000
            // to print log characters. Without this mapping, a Kernel Data Abort (ESR_EL1=0x96000045, FAR_EL1=0x09000000)
            // occurs inside the sync_el0 / irq vector handlers, leading to double-fault locking.
            let uart_flags = crate::arch::aarch64::mmu::MapFlags::device_rw();
            if let Err(e) = crate::arch::aarch64::mmu::map_page_under_l0(l0_user_pa, 0x09000000, 0x09000000, uart_flags) {
                crate::kprintln!("WARNING: Failed to map UART under user L0: {:?}", e);
            }

            let mut ttbr0_reg: u64;
            core::arch::asm!("mrs {0}, ttbr0_el1", out(reg) ttbr0_reg, options(nomem, nostack));
            old_ttbr0 = (ttbr0_reg & 0x0000_FFFF_FFFF_F000) as usize;
        }
        for &b in b"LAUNCHER OK fresh L0\n" {
            crate::arch::console_putchar(b);
        }
        for &b in b"[DIAG] about to map segments\n" {
            crate::arch::console_putchar(b);
        }

        let mut lowest_vaddr: usize = usize::MAX;
        for &b in b"[DIAG] entering segment loop\n" {
            crate::arch::console_putchar(b);
        }

        for idx in 0..header.header_count {
            for &b in b"[DIAG] get_entry start\n" {
                crate::arch::console_putchar(b);
            }
            let entry_meta = match parser.get_entry(idx) {
                Ok(e) => e,
                _ => {
                    for &b in b"[DIAG] get_entry err\n" {
                        crate::arch::console_putchar(b);
                    }
                    continue;
                }
            };
            for &b in b"[DIAG] get_entry OK\n" {
                crate::arch::console_putchar(b);
            }

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
            for &b in b"[DIAG] seg ty=" {
                crate::arch::console_putchar(b);
            }
            crate::arch::console_putchar(b'0' + ty as u8);
            for &b in b" tgt_va=0x" {
                crate::arch::console_putchar(b);
            }
            let mut tmp = target_va;
            for i in (0..16).rev() {
                let nibble = (tmp >> (i * 4)) & 0xF;
                crate::arch::console_putchar(if nibble < 10 { b'0' + nibble as u8 } else { b'A' + (nibble - 10) as u8 });
            }
            crate::arch::console_putchar(b'\n');

            let mut vmo = Vmo::create_with_size(aligned_size)?;
            for &b in b"[DIAG] vmo created\n" {
                crate::arch::console_putchar(b);
            }
            vmo.commit_all()?;
            for &b in b"[DIAG] vmo committed\n" {
                crate::arch::console_putchar(b);
            }
            for &b in b"[DIAG] about to vmar.map\n" {
                crate::arch::console_putchar(b);
            }
            if entry_meta.file_size > 0 {
                let segment_payload = parser.get_segment_data(&entry_meta).map_err(|e| {
                    crate::log_error!("LAUNCHER", "Failed to retrieve segment payload: {:?}", e);
                    Status::InvalidArgs
                })?;
                let write_offset = alignment_offset;
                vmo.write(write_offset, segment_payload)?;
            }

            proc.root_vmar.map_under_l0(&mut vmo, 0, target_va, aligned_size, flags, l0_user_pa)?;
            for &b in b"[DIAG] vmar.map OK\n" {
                crate::arch::console_putchar(b);
            }
            #[cfg(target_arch = "aarch64")]
            crate::arch::aarch64::mmu::sync_instruction_cache(target_va, aligned_size);
        }
        for &b in b"[DIAG] segment loop done\n" {
            crate::arch::console_putchar(b);
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
        for &b in b"[DIAG] creating stack vmo\n" {
            crate::arch::console_putchar(b);
        }
        let mut stack_vmo = Vmo::create_with_size(stack_size)?;
        stack_vmo.commit_all()?;
        for &b in b"[DIAG] stack vmo committed\n" {
            crate::arch::console_putchar(b);
        }
        let stack_vaddr_offset = 0x2000000;
        let stack_va = proc.root_vmar.base + stack_vaddr_offset;

        let stack_flags = VmarFlags::from_bits(
            VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits()
        );

        proc.root_vmar.map_under_l0(&mut stack_vmo, 0, stack_va, stack_size, stack_flags, l0_user_pa)?;
        for &b in b"[DIAG] stack vmar.map done\n" {
            crate::arch::console_putchar(b);
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
            // Take a known-safe stack page address (top - 16,
            // which is well inside the 16 KiB region).
            let touch_va = stack_top - 16;
            let touch_l0_pa = l0_user_pa;
            if let Some(touch_pa) = crate::arch::aarch64::mmu::translate_user_va(
                touch_l0_pa,
                touch_va,
            ) {
                let touch_kernel_va = crate::mm::mmu::pa_to_kernel_va(touch_pa) as *mut u64;
                unsafe {
                    // 8-byte volatile store so QEMU-TCG
                    // materialises the backing RAM.  The
                    // value is 0 (same as the page's existing
                    // content) so this is a coherent write
                    // that does not perturb the stack.
                    core::ptr::write_volatile(touch_kernel_va, 0u64);
                    // dsb ish makes the write observable to
                    // the page-table walker before the next
                    // TLB-fill.
                    core::arch::asm!("dsb ish", options(nomem, nostack));
                }
            }
        }

        use crate::mm::mmu::ArchMmu;
        crate::kprintln!("[DIAG] about to flush_tlb_all");
        #[cfg(target_arch = "aarch64")]
        crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();
        crate::kprintln!("[DIAG] flush_tlb_all done");

        let user_entry = proc.root_vmar.base + user_entry_offset;
        for &b in b"[DIAG] user_entry=0x" {
            crate::arch::console_putchar(b);
        }
        let mut tmp = user_entry;
        for i in (0..16).rev() {
            let nibble = (tmp >> (i * 4)) & 0xF;
            crate::arch::console_putchar(if nibble < 10 { b'0' + nibble as u8 } else { b'A' + (nibble - 10) as u8 });
        }
        crate::arch::console_putchar(b'\n');
        #[cfg(target_arch = "aarch64")]
        if let Some(pa) = crate::arch::aarch64::mmu::translate_user_va(l0_user_pa, user_entry) {
            for &b in b"[DIAG] user_entry->PA=0x" {
                crate::arch::console_putchar(b);
            }
            let mut tmp = pa;
            for i in (0..16).rev() {
                let nibble = (tmp >> (i * 4)) & 0xF;
                crate::arch::console_putchar(if nibble < 10 { b'0' + nibble as u8 } else { b'A' + (nibble - 10) as u8 });
            }
            crate::arch::console_putchar(b'\n');
        } else {
            for &b in b"[DIAG] user_entry TRANSLATE FAILED\n" {
                crate::arch::console_putchar(b);
            }
        }

        let mut thread = Thread::new_user(name, user_entry, stack_top)?;
        crate::kprintln!("[DIAG] Thread::new_user done, thread_id={}", thread.id);
        thread.process_id = pid;
        thread.context.process_id = pid;
        thread.context.l0_user_pa = l0_user_pa as u64;
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
            if let Some(sp_top_pa_resolved) = crate::arch::aarch64::mmu::translate_user_va(
                l0_user_pa,
                sp_top_aligned,
            ) {
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

        Ok(())
    }
}

pub const MAX_PROCESSES: usize = 8;
pub static mut PROCESSES: [Option<Process>; MAX_PROCESSES] = [None, None, None, None, None, None, None, None];

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
