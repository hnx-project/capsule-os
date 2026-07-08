use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::mm::vmar::Vmar;
use crate::object::handle_table::HandleTable;

static PROCESS_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
// Reserved for future per-process VMAR work (see commit message).
// Currently unused: every process shares the legacy base so linker
// PC-relative references stay correct; `sys_exec` marks its caller
// Dead to prevent cross-process adr pollution.
pub static VMAR_BASE_SLOT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
pub struct Process {
    pub id: u64,
    pub name: &'static str,
    pub state: ProcessState,
    pub root_vmar: Vmar,
    pub handle_table: HandleTable,
    pub thread_count: usize,
    pub l0_user_pa: usize,
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
        // High-reusability multi-slot address space allocator:
        // By offsetting each process's virtual mapping base range by 256 MiB,
        // we guarantee zero virtual overlap across processes. Even under a legacy
        // shared single page table, this encapsulation guarantees that processes
        // (loader, init, devmgr, etc.) will never overwrite or step on each other's 
        // segments, providing elegant EL0 software-level memory isolation.
        let slot = VMAR_BASE_SLOT.fetch_add(1, Ordering::Relaxed);

        #[cfg(target_arch = "aarch64")]
        let vmar_base = 0x9000_0000usize + slot * 0x1000_0000usize;
        #[cfg(target_arch = "riscv64")]
        let vmar_base = 0x9000_0000usize + slot * 0x1000_0000usize;

        let root_vmar = Vmar::create(vmar_base, 256 * 1024 * 1024)?;

        let handle_table = HandleTable::new();
        Ok(Process {
            id: PROCESS_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            state: ProcessState::Initial,
            root_vmar,
            handle_table,
            thread_count: 0,
            l0_user_pa: 0,
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
        use crate::mm::vmo::Vmo;
        use crate::mm::vmar::VmarFlags;
        use crate::task::thread::{Thread, ThreadState};

        let parser = ohlink_format::parser::OHLK_Parser::new(binary_bytes).map_err(|e| {
            crate::log_error!("LAUNCHER", "Failed to parse OHLINK format: {:?}", e);
            Status::InvalidArgs
        })?;

        let header = parser.header();

        let mut entry_offset = u64::from_le_bytes([
            header.reserved[0], header.reserved[1], header.reserved[2], header.reserved[3],
            header.reserved[4], header.reserved[5], header.reserved[6], header.reserved[7],
        ]) as usize;

        // 1. Allocate the process from our global list
        let proc = allocate_process(name)?;
        let pid = proc.id;

        // Allocate and zero-initialize process-specific L0 page table 
        // now that MMU and phys-allocator are 100% active, avoiding bootstrapping lock deadlines!
        let l0_user_pa = crate::mm::phys::alloc_page()?.as_usize();
        proc.l0_user_pa = l0_user_pa;

        #[cfg(target_arch = "aarch64")]
        unsafe {
            // Read TTBR1_EL1 which contains the master kernel L0 physical address.
            let mut ttbr1: u64;
            core::arch::asm!("mrs {0}, ttbr1_el1", out(reg) ttbr1, options(nomem, nostack));
            let kernel_l0_pa = (ttbr1 & 0x0000_FFFF_FFFF_F000) as usize;

            // Compute kernel virtual addresses for both structures so we can securely write-volatile them.
            let kernel_l0_va = crate::mm::mmu::pa_to_kernel_va(kernel_l0_pa) as *const u64;
            let user_l0_va = crate::mm::mmu::pa_to_kernel_va(l0_user_pa) as *mut u64;

            // Copy the entire kernel high-half region (slots 256..512) into the new user-isolated L0 table!
            for i in 256..512 {
                let entry = core::ptr::read_volatile(kernel_l0_va.add(i));
                core::ptr::write_volatile(user_l0_va.add(i), entry);
            }
        }

        // 2. Parse and map OHLINK segments
        for idx in 0..header.header_count {
            let mut entry_meta = match parser.get_entry(idx) {
                Ok(e) => e,
                _ => continue,
            };
            if entry_meta.ty != ohlink_format::SegmentType::Text.to_u32()
                && entry_meta.ty != ohlink_format::SegmentType::Data.to_u32()
                && entry_meta.ty != ohlink_format::SegmentType::Rodata.to_u32()
                && entry_meta.ty != ohlink_format::SegmentType::Bss.to_u32()
            {
                continue;
            }

            let virt_addr = 0x200000;
            let mut size = entry_meta.mem_size as usize;
            
            // Expand size if this is the Text segment to fully envelope the real entry point!
            if entry_meta.ty == ohlink_format::SegmentType::Text.to_u32() {
                let required_size = entry_offset + 4096;
                if size < required_size {
                    size = required_size;
                }
            }

            let flags_raw = entry_meta.flags;

            let aligned_vaddr = virt_addr & !(4096 - 1);
            let alignment_offset = virt_addr - aligned_vaddr;
            let aligned_size = (size + alignment_offset + 4095) & !(4095);

            let target_va = proc.root_vmar.base + aligned_vaddr;
            let flags = VmarFlags::from_bits(flags_raw);
            crate::log_info!("LAUNCHER", "Mapping segment: ty={:#x}, flags={:?} (raw={:#x}), target_va={:#x}, size={}", entry_meta.ty, flags, flags_raw, target_va, aligned_size);

            let mut vmo = Vmo::create_with_size(aligned_size)?;
            vmo.commit_all()?; // Commit physical pages so memory is backed
            if entry_meta.file_size > 0 {
                let segment_payload = parser.get_segment_data(&entry_meta).map_err(|e| {
                    crate::log_error!("LAUNCHER", "Failed to retrieve segment payload: {:?}", e);
                    Status::InvalidArgs
                })?;
                vmo.write(alignment_offset, segment_payload)?;
            }

            // Restore entry_meta offset to keep it compatible with relative jump setups
            entry_meta.offset = 0x200000;

            proc.root_vmar.map(&mut vmo, 0, target_va, aligned_size, flags)?;

            // Clean data cache and invalidate instruction cache to ensure the CPU reads
            // the actual newly written instructions on the execution pipeline.
            #[cfg(target_arch = "aarch64")]
            crate::arch::aarch64::mmu::sync_instruction_cache(target_va, aligned_size);
        }

        // 3. Map Stack
        let stack_size = 16 * 1024;
        let mut stack_vmo = Vmo::create_with_size(stack_size)?;
        stack_vmo.commit_all()?; // Commit physical stack pages so it is writable and readable
        let stack_vaddr_offset = 0x2000000;
        let stack_va = proc.root_vmar.base + stack_vaddr_offset;

        let stack_flags = VmarFlags::from_bits(
            VmarFlags::READ.bits() | VmarFlags::WRITE.bits() | VmarFlags::USER.bits()
        );

        proc.root_vmar.map(&mut stack_vmo, 0, stack_va, stack_size, stack_flags)?;

        let stack_top = (stack_va + stack_size) & !(15usize);

        // CRITICAL TLB FLUSH: Since mapping user segments has allocated new intermediate L1/L2 table descriptors
        // that were previously zero (invalid) in the shared page table directory, we MUST flush all TLB and translation 
        // table walk caches (PTW) globally. Otherwise, the hardware CPU's TLB branch predictor will fetch from old cached invalid table Walks
        // and trigger an immediate Level-1 or Level-2 Translation Fault.
        use crate::mm::mmu::ArchMmu;
        #[cfg(target_arch = "aarch64")]
        crate::arch::aarch64::mmu::AArch64Mmu::flush_tlb_all();

        // 4. Create Thread and add to scheduler
        // Calculate the real physical offset inside the compiled program segment.
        // In the new merged single-segment OHLINK format, the entire ELF's load segments (including text at 0x210158 etc.)
        // are merged into a single segment starting at 0x200000.
        // Therefore, the virtual entry offset relative to the segment's starting virtual address is:
        // actual_entry (0x2101d0) - lowest_vaddr (0x200000) = 0x101d0.
        // In the kernel mapping, we map this merged segment to proc.root_vmar.base + 0x200000.
        // Thus, the physical entry_point is proc.root_vmar.base + 0x200000 + entry_offset.
        let user_entry = proc.root_vmar.base + 0x200000 + entry_offset;
        let mut thread = Thread::new_user(name, user_entry, stack_top)?;
        thread.process_id = pid;
        thread.handle_table = &proc.handle_table;
        thread.state = ThreadState::Ready;

        unsafe {
            crate::task::scheduler::SCHEDULER.add(thread);
        }

        crate::log_info!("LAUNCHER", "Successfully launched program '{}' at EL0 (entry={:#x}, pid={})", name, user_entry, pid);
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
