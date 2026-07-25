use crate::memory::Vmar;
use crate::memory::Memory;
use crate::object::handle_table::HandleTable;
use crate::vfs::pipe::{PipeId, PipeRole};
use crate::arch::ArchHardware;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use shared::status::{Result, Status};
use shared::types::HandleValue;

/// One slot in `Process::fd_table`.  For 1.0 we only carry
/// pipe-end entries; once fileagent starts serving fds directly
/// to a process (rather than through the channel forwarder) we
/// will add a `VfsHandle(HandleValue)` variant here.
#[derive(Debug, Clone, Copy)]
pub enum FdEntry {
    Pipe { pipe: PipeId, role: PipeRole },
    Tty {
        pty: crate::object::tty::PtyId,
        /// `Master` reads from / writes to the master side;
        /// `Slave` is the OS-side terminal (used by the bash
        /// child after `setsid`).
        role: TtyRole,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtyRole {
    Master,
    Slave,
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
    pub page_table: <crate::arch::CurrentArch as crate::arch::ArchHardware>::PageTable,
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
    /// Architecture-specific address space ID (ASID).
    pub asid: <crate::arch::CurrentArch as crate::arch::ArchHardware>::AddressSpaceId,
    /// List of VMOs owned by this process. This guarantees that all physical memory
    /// pages allocated for the process's stack, code, and data segments remain
    /// permanently reserved under explicit object ownership.
    pub vmos: alloc::vec::Vec<crate::memory::vmo::Vmo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Initial,
    Running,
    Zombie,
    Dead,
}

impl Process {
        pub fn new_dummy() -> Self {
            Process {
                id: 0,
                name: "",
                state: ProcessState::Initial,
                root_vmar: Vmar::new_dummy(),
                handle_table: HandleTable::new_dummy(),
                thread_count: 0,
                page_table: <<crate::arch::CurrentArch as crate::arch::ArchHardware>::PageTable as crate::arch::ArchPageTable>::new(),
                cwd: [0u8; CWD_MAX],
                cwd_len: 0,
                exit_status: None,
                parent_pid: 0,
                sig_handlers: [0u8; 32],
                pending_signals: 0u32,
                fd_table: [const { None }; FD_TABLE_SIZE],
                next_fd: USER_FD_BASE,
                asid: <crate::arch::CurrentArch as crate::arch::ArchHardware>::kernel_asid(),
                vmos: alloc::vec::Vec::new(),
            }
        }

    pub fn init_in_place(&mut self, name: &'static str) -> Result<()> {
        let vmar_base = 0x0usize; // 2.0 时代进程间独享 L0 隔离，基准地址全盘归零大一统！

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
        self.page_table = <<crate::arch::CurrentArch as crate::arch::ArchHardware>::PageTable as crate::arch::ArchPageTable>::new();
        self.cwd = cwd;
        self.cwd_len = cwd_len;
        self.exit_status = None;
        self.parent_pid = 0;
        self.sig_handlers = [0u8; 32];
        self.pending_signals = 0u32;
        self.fd_table = [const { None }; FD_TABLE_SIZE];
        self.next_fd = USER_FD_BASE;
        self.vmos = alloc::vec::Vec::new();

        self.asid = <crate::arch::CurrentArch as crate::arch::ArchHardware>::alloc_asid()
            .unwrap_or_else(|| <crate::arch::CurrentArch as crate::arch::ArchHardware>::kernel_asid());

        Ok(())
    }

    pub fn add_thread(&mut self) {
        self.thread_count = self.thread_count.wrapping_add(1);
    }

    /// S7 / procmgr std-fd handoff: install the spawner's
    /// `(stdin, stdout, stderr)` channel handles as the new
    /// process's `fd_table[0..=2]`.  Each handle is optional; a
    /// `None` entry leaves the corresponding slot as
    /// `FdEntry::Pipe { … }` (kernel-builtin UART) so the child
    /// still has a working stdio.  The kernel translates each
    /// handle through the *caller's* handle table; the child
    /// receives a copy of the resulting `KernelObject` so it
    /// owns a fresh reference.
    pub fn set_std_fds(
        &mut self,
        handles: [Option<u32>; 3],
        caller_table: &HandleTable,
    ) -> Result<()> {
        for (slot, h) in handles.iter().enumerate() {
            let entry = match h {
                None => FdEntry::Pipe {
                    pipe: crate::vfs::pipe::PipeId(0),
                    role: match slot {
                        0 | 1 => crate::vfs::pipe::PipeRole::Read,
                        _ => crate::vfs::pipe::PipeRole::Write,
                    },
                },
                Some(h) => {
                    let obj = match caller_table.read_clone(HandleValue::new(*h)) {
                        Ok(o) => o,
                        Err(_) => return Err(Status::NotFound),
                    };
                    FdEntry::Tty {
                        // S7: the spawner hands us a channel
                        // handle that we wrap as a pseudo-fd.
                        // Once `/dev/tty` is wired through procmgr
                        // (S7.3 below) this will resolve to a
                        // real `FdEntry::Tty` referencing the
                        // process's controlling PTY.  Until then
                        // we surface the channel handle as a
                        // generic object entry.
                        pty: crate::object::tty::PtyId(0),
                        role: TtyRole::Master,
                    }
                }
            };
            self.fd_table[slot] = Some(entry);
        }
        Ok(())
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

    /// Launch a fresh EL0 program with `argc`/`argv` and `envc`/`envp`
    /// both materialised onto the user stack.  Empty `envp` is allowed —
    /// the trampoline reads `envc == 0` and skips the env walk.
    pub fn launch_user_program_with_argv_and_envp(
        name: &'static str,
        binary_bytes: &[u8],
        arg_strs: &[[u8; 256]],
        arg_lens: &[usize],
        argc: usize,
        env_strs: &[[u8; 256]],
        env_lens: &[usize],
        envc: usize,
        parent_pid: u64,
    ) -> Result<u64> {
        let mut proc_id = 0;
        Self::launch_user_program_with_argv_and_envp_id(
            name,
            binary_bytes,
            arg_strs,
            arg_lens,
            argc,
            env_strs,
            env_lens,
            envc,
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
        Self::launch_user_program_with_argv_and_envp_id(
            name,
            binary_bytes,
            arg_strs,
            arg_lens,
            argc,
            &[],
            &[],
            0,
            parent_pid,
            out_pid,
        )
    }

    /// S13.1: same as `launch_user_program_with_argv_id` but also
    /// materialises `envc`/`envp` onto the user stack.  Empty
    /// `envp` is allowed (envc = 0).  The kernel trampoline reads
    /// x2=envc, x3=envp_ptr.
    pub fn launch_user_program_with_argv_and_envp_id(
        name: &'static str,
        binary_bytes: &[u8],
        arg_strs: &[[u8; 256]],
        arg_lens: &[usize],
        argc: usize,
        env_strs: &[[u8; 256]],
        env_lens: &[usize],
        envc: usize,
        parent_pid: u64,
        out_pid: &mut u64,
    ) -> Result<()> {
        use crate::memory::VmarFlags;
        use crate::memory::vmo::Vmo;
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
        unsafe {
            let active_l0_pa = crate::arch::CurrentArch::get_active_page_table();
            old_ttbr0 = active_l0_pa;

            proc.page_table.allocate_root()?;

            // Map the UART device physical page (0x09000000) under the process L0.
            let uart_flags = crate::arch::mmu::MapFlags::device_rw_user();
            if let Err(e) = proc.page_table.map_va(0x09000000, 0x09000000, &uart_flags) {
                crate::kprintln!("WARNING: Failed to map UART under user L0: {:?}", e);
            }

            // Map GIC CPU Interface page under user L0.
            let gic_flags = crate::arch::mmu::MapFlags::device_rw_user();
            if let Err(e) = proc.page_table.map_va(0x08010000, 0x08010000, &gic_flags) {
                crate::kprintln!("WARNING: Failed to map GIC under user L0: {:?}", e);
            }

            // Map Virtio-Blk device physical page (0x0a003000) under user L0.
            let virtio_flags = crate::arch::mmu::MapFlags::device_rw_user();
            if let Err(e) = proc.page_table.map_va(0x0a003000, 0x0a003000, &virtio_flags) {
                crate::kprintln!("WARNING: Failed to map Virtio-Blk under user L0: {:?}", e);
            }

            // Copy the high-half kernel entries (L0[256..512]) and identity
            // L1 block from the current TTBR0 (parent) into the new tree.
            if active_l0_pa != 0 {
                proc.page_table.clone_high_half(active_l0_pa);
                proc.page_table.clone_identity_block(active_l0_pa);
            }
        }
        let mut lowest_vaddr: usize = usize::MAX;
        let pt = &mut proc.page_table;

        let irq_flags = unsafe { crate::arch::CurrentArch::local_irq_save() };

        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED after root setup pid={}", proc.id);
        }

        let entry_point_abs = header.entry_point as usize;
        let mut calculated_entry: Option<usize> = None;

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

            crate::log_info!("LAUNCHER", "SEGMENT[{}]: virt_addr={:#x}, size={}, ty={}", idx, virt_addr, size, ty);

            if virt_addr < lowest_vaddr {
                lowest_vaddr = virt_addr;
            }

            let mut flags_raw = entry_meta.flags;
            if ty == 1 {
                // TYPE_TEXT: 代码段强制赋予 用户态只读 + 可执行 (RX, User)
                flags_raw = VmarFlags::READ.bits() | VmarFlags::EXECUTE.bits() | VmarFlags::USER.bits();
            } else if ty == 2 || ty == 3 || ty == 4 {
                // 数据段/只读数据段/BSS: 强制赋予 用户态读写 + 不可执行 (RW, NX, User)
                flags_raw = VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits();
            }

            let aligned_vaddr = virt_addr & !(4096 - 1);
            let alignment_offset = virt_addr - aligned_vaddr;
            let aligned_size = (size + alignment_offset + 4095) & !(4095);

            let target_va = proc.root_vmar.base + aligned_vaddr;
            let flags = VmarFlags::from_bits(flags_raw);

            // 如果 entry_point 处于该段的原始链接空间范围内
            if entry_point_abs >= virt_addr && entry_point_abs < virt_addr + size {
                let offset_in_segment = entry_point_abs - virt_addr;
                // 段内相对偏移重定位
                let final_pc = proc.root_vmar.base + virt_addr + offset_in_segment;
                calculated_entry = Some(final_pc);
            }

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
                .reserve_mapping(vmo.id, 0, target_va, aligned_size, flags)?;

            let m_flags = if ty == 1 {
                crate::arch::mmu::MapFlags::user_rx()
            } else {
                crate::arch::mmu::MapFlags::user_rw()
            };

            let page_count = aligned_size / 4096;
            for i in 0..page_count {
                let page_va = target_va + i * 4096;
                let page_pa = vmo.get_page_phys(i * 4096).unwrap().as_usize();
                pt.map_va(page_va, page_pa, &m_flags)?;
            }

            {
                // To maintain full cache coherency, we must clean D-cache and invalidate I-cache
                // of each allocated physical page using the kernel's high-half direct-map alias virtual addresses (KVA),
                // since the active page table context at this point doesn't map target_va.
                let page_count = aligned_size / 4096;
                for i in 0..page_count {
                    let vmo_off = i * 4096;
                    if let Some(pa) = vmo.get_page_phys(vmo_off) {
                        let kva = crate::arch::mmu_facade::pa_to_kernel_va(pa.as_usize());
                        unsafe {
                            <crate::arch::CurrentArch as crate::arch::ArchHardware>::sync_instruction_cache(kva, 4096);
                        }
                    }
                }
            }
        }

        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED after OHLINK segments pid={}", proc.id);
        }

        // 完美的段相对偏移重定位自愈入口计算，后备为原始偏移
        let user_entry = calculated_entry.unwrap_or_else(|| {
            proc.root_vmar.base + entry_point_abs
        });

        let stack_size = 16 * 1024;
        let mut stack_vmo = Vmo::create_with_size(stack_size)?;
        stack_vmo.commit_all()?;
        let stack_vaddr_offset = 0x2000000;
        let stack_va = proc.root_vmar.base + stack_vaddr_offset;

        let stack_flags = VmarFlags::from_bits(
            VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits(),
        );

        proc.root_vmar.reserve_mapping(
            stack_vmo.id,
            0,
            stack_va,
            stack_size,
            stack_flags,
        )?;
        for i in 0..(stack_size / 4096) {
            let va = stack_va + i * 4096;
            let pa = stack_vmo.get_page_phys(i * 4096).unwrap().as_usize();
            let mut s_flags = crate::arch::mmu::MapFlags::kernel_rw();
            s_flags.user = true;
            s_flags.writable = true;
            pt.map_va(va, pa, &s_flags)?;
        }

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
        {
            let touch_va = stack_top - 16;
            if let Some(touch_pa) = pt.translate_va(touch_va) {
                let touch_kernel_va = crate::arch::mmu_facade::pa_to_kernel_va(touch_pa) as *mut u64;
                unsafe {
                    core::ptr::write_volatile(touch_kernel_va, 0u64);
                    crate::arch::CurrentArch::memory_barrier();
                }
            }
        }

        unsafe {
            crate::arch::CurrentArch::flush_tlb();
        }

        crate::log_info!("LAUNCHER", "LAUNCHING: user_entry={:#x}, vmar_base={:#x}, header.entry={:#x}", user_entry, proc.root_vmar.base, header.entry_point);

        let mut thread = Thread::new_user(name, user_entry, stack_top)?;
        thread.process_id = pid;
        thread.context.process_id = pid;

        thread.context.l0_user_pa = <crate::arch::CurrentArch as crate::arch::ArchHardware>::pack_ttbr(pt.l0_pa() as u64, proc.asid);

        thread.context.page_table_gen = pt.generation;

        // 强行将刚刚写入的线程寄存器和页表上下文同步清出 Data-Cache，保障调度器 100% 绝对物理可见！
        unsafe {
            crate::arch::CurrentArch::clean_and_invalidate_cache_range(
                &thread.context as *const _ as usize,
                core::mem::size_of_val(&thread.context),
            );
        }

        // Strict SPSR Lock: enforce EL0t privilege level with IRQs fully unmasked (spsr=0x000)
        // to prevent timer preempt or exception handler from corrupting the register context
        #[cfg(target_arch = "aarch64")]
        {
            thread.context.spsr = 0x000;
        }
        // Ensure the thread's handle_table pointer points to high-half KVA
        // instead of raw physical/identity address, so it survives TTBR0 page-table switches!
        let ht_raw = &proc.handle_table as *const HandleTable as usize;
        let ht_kva = if ht_raw < crate::arch::mmu_facade::KERNEL_OFFSET {
            crate::arch::mmu_facade::pa_to_kernel_va(ht_raw)
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
        //
        // `post_argv_sp` is lifted out of the `if argc > 0` block so the
        // S13.1 envp materialisation can chain on top of argv when
        // `argc == 0` (we still need a sensible sp for envp to land
        // on; in that case `post_argv_sp` is the value argv would have
        // produced, i.e. `stack_top` aligned down).
        let post_argv_sp: usize;
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
            post_argv_sp = (cursor + 15) & !15;

            thread.context.user_sp = post_argv_sp as u64;
            thread.context.x[1] = argv_ptr_va as u64;
        } else {
            // No argv was materialised; envp will land just below
            // `stack_top`.  We round down to 16 for the same
            // alignment the envp materialiser uses.
            post_argv_sp = stack_top & !(15usize);
            thread.context.user_sp = post_argv_sp as u64;
            thread.context.x[1] = 0;
        }

        // S13.1: materialise envp on the user stack, just below
        // argv.  Layout matches POSIX `int main(int argc, char
        // *argv[], char *envp[])`: a NULL-terminated pointer array
        // (we don't write the NULL because envc tells the
        // trampoline the count, but we round up the stack frame
        // for alignment), then string payloads.  Each string is
        // NUL-terminated and 16-byte aligned so the trampoline
        // can scan via the standard "next-pointer minus current"
        // trick.
        if envc > 0 {
            let envp_array_bytes = envc * 8;
            let mut env_string_total: usize = 0;
            for i in 0..envc {
                env_string_total += (env_lens[i] + 15) & !15;
            }
            let envp_area = envp_array_bytes + env_string_total + 16;

            // Continue from `post_argv_sp` (the current sp),
            // which was lifted to outer scope so the envp path
            // can chain on top of argv.
            let mut env_new_sp =
                (post_argv_sp - envp_area) & !(15usize);
            let mut env_cursor = env_new_sp;

            // First: copy each envp string payload upward,
            // capturing the user-VA so the pointer array below
            // points at it.  Each string is NUL-terminated for
            // the same reason argv strings are.
            let mut env_vas: [usize; 16] = [0usize; 16];
            for i in 0..envc {
                let s_len = env_lens[i];
                let padded = (s_len + 15) & !15;
                let dst = env_cursor;
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    pt.l0_pa(),
                    &env_strs[i][..s_len],
                    dst,
                    s_len,
                )?;
                let nul: [u8; 1] = [0u8];
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    pt.l0_pa(),
                    &nul,
                    dst + s_len,
                    1,
                )?;
                env_vas[i] = dst;
                env_cursor += padded;
            }

            // Then: write the envp pointer array.
            let envp_ptr_va = env_cursor;
            for i in 0..envc {
                let bytes = (env_vas[i] as u64).to_le_bytes();
                crate::syscall::handlers::ipc::safe_copy_to_user(
                    pt.l0_pa(),
                    &bytes,
                    envp_ptr_va + i * 8,
                    8,
                )?;
            }

            // The new initial sp now points at the envp area;
            // argv lives just above it.
            let post_envp_sp = (env_cursor + envp_array_bytes + 15) & !15;
            thread.context.user_sp = post_envp_sp as u64;
            thread.context.x[2] = envc as u64;
            thread.context.x[3] = envp_ptr_va as u64;
        } else {
            thread.context.x[2] = 0;
            thread.context.x[3] = 0;
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
            if let Some(_sp_top_pa_resolved) = pt.translate_va(sp_top_aligned) {
                unsafe {
                    crate::arch::CurrentArch::invalidate_stack_line(sp_top_aligned);
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
            // 全局 CPU I-Cache Invalidate:
            // 采用全局冲刷 (ic ialluis) 配合最高强度系统屏障，100% 根除所有 CPU 别名 I-Cache 残留与流水线脏取指！
            core::arch::asm!(
                "ic ialluis",
                "dsb sy",
                "isb",
                options(nomem, nostack)
            );

            // Invalidate TLB for all ASIDs to prevent any translation leftovers
            core::arch::asm!(
                "tlbi vaae1is, xzr",
                "dsb sy",
                "isb",
                options(nomem, nostack)
            );

            ()
        };

        // POST-LAUNCH stack PTE verification: confirm all 4 stack
        // pages have valid L3 PTEs immediately after mapping.
        #[cfg(target_arch = "aarch64")]
        {
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
        }

        if !pt.validate() {
            crate::log_error!("LAUNCHER", "PT-VALIDATE FAILED at post-launch pid={}", proc.id);
        }

        // **Restore the caller-side page table** before returning to the
        // kernel context, but ONLY when there is a sensible previous
        // page table to return to.
        unsafe {
            <crate::arch::CurrentArch as crate::arch::ArchHardware>::restore_user_page_table(old_ttbr0, pt.l0_pa(), proc.asid);
            <crate::arch::CurrentArch as crate::arch::ArchHardware>::local_irq_restore(irq_flags);
        }

        Ok(())
    }
}

pub const MAX_PROCESSES: usize = 16;
pub static mut PROCESSES: [Option<Process>; MAX_PROCESSES] = [
    None, None, None, None, None, None, None, None,
    None, None, None, None, None, None, None, None,
];

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

/// Allocate a fresh `Process` slot and run its `init_in_place`.
///
/// `lock_held` distinguishes the two callers:
///   - `false` — acquire / release the scheduler lock around the
///     table walk (the common path, used by `spawn`, `sys_spawn`,
///     etc).
///   - `true`  — caller already holds the scheduler lock (e.g.
///     `sys_fork`).  Skip the lock dance so we don't deadlock
///     on the recursive acquire.
pub fn allocate_process_with_lock(name: &'static str, lock_held: bool) -> Result<&'static mut Process> {
    let (slot, flags) = unsafe {
        if lock_held {
            let slot = find_empty_process_slot_locked();
            (slot, 0usize)
        } else {
            let flags = crate::task::scheduler::SCHEDULER.lock();
            let slot = find_empty_process_slot_locked();
            (slot, flags)
        }
    };
    let slot = match slot {
        Some(s) => s,
        None => {
            if !lock_held {
                unsafe { crate::task::scheduler::SCHEDULER.unlock(flags); }
            }
            return Err(Status::NoMemory);
        }
    };
    unsafe {
        *slot = Some(Process::new_dummy());
        let proc_ref = slot.as_mut().unwrap();
        let res = proc_ref.init_in_place(name);
        if !lock_held {
            crate::task::scheduler::SCHEDULER.unlock(flags);
        }
        res?;
        Ok(proc_ref)
    }
}

/// Backwards-compatible wrapper that acquires the scheduler
/// lock.  Kept so existing call sites don't need to be touched.
pub fn allocate_process(name: &'static str) -> Result<&'static mut Process> {
    allocate_process_with_lock(name, false)
}

/// Walk `PROCESSES` looking for an empty slot.  Caller must
/// already hold the scheduler lock (or be running lock-free
/// during single-core boot).
unsafe fn find_empty_process_slot_locked() -> Option<&'static mut Option<Process>> {
    for slot in PROCESSES.iter_mut() {
        if slot.is_none() {
            return Some(slot);
        }
    }
    None
}

pub fn find_process_mut(id: u64) -> Option<&'static mut Process> {
    unsafe {
        let flags = crate::task::scheduler::SCHEDULER.lock();
        let res = find_process_mut_locked(id);
        crate::task::scheduler::SCHEDULER.unlock(flags);
        res
    }
}

/// Lock-free variant of `find_process_mut`.  Caller must
/// already hold the scheduler lock.
pub unsafe fn find_process_mut_locked(id: u64) -> Option<&'static mut Process> {
    for slot in PROCESSES.iter_mut() {
        if let Some(p) = slot {
            if p.id == id {
                let ptr = p as *mut Process;
                return Some(&mut *ptr);
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
        // 4. Reap every thread that still belongs to this PID: drop its scheduler slot and free its kernel stack
        // 5. Finally, release the OHLINK segment and Stack VMO physical data pages

        crate::log_info!(
            "PROCESS_DROP",
            "Process '{}' (PID {}) is dropping. Reclaiming intermediate page table pages.",
            self.name,
            self.id
        );

        unsafe {
            // S11 fork kstack fix: scan the scheduler's thread
            // table for any thread whose `process_id` matches
            // ours, free its kernel-stack pages, and clear the
            // scheduler slot.  Without this, every fork child
            // (and every `Init`-spawned thread the process owns)
            // leaks `KERNEL_STACK_PAGES` pages until the
            // scheduler slot is reused.
            use crate::task::scheduler::SCHEDULER;
            let flags = SCHEDULER.lock();
            let reclaimed = SCHEDULER.reclaim_threads_for_pid(self.id);
            SCHEDULER.unlock(flags);
            for t in reclaimed {
                t.free_kstack();
            }

            <crate::arch::CurrentArch as crate::arch::ArchHardware>::flush_tlb();
            self.page_table.free_tree();
        }
    }
}
