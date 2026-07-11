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
        Self::launch_user_program_with_argv(name, binary_bytes, &[], &[], 0)
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
            // Cache maintenance on the L0 entries we just wrote --
            // without it, a later MMU walk through TTBR0_EL1 can read
            // a stale entry from the data cache while the new bytes
            // are still sitting in a write buffer.
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
            // "scan until 0 byte" loop (in `userspace/hnxlibc/src/lib.rs`
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
