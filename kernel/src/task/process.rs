use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use shared::status::{Result, Status};
use crate::mm::vmar::Vmar;
use crate::object::handle_table::HandleTable;

static PROCESS_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
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

        // entry_point in OHLINK header now represents the absolute virtual address
        // (lowest_vaddr + entry_offset_in_merged) where the user program should start.
        let entry_offset = if lowest_vaddr != usize::MAX && header.entry_point as usize >= lowest_vaddr
        {
            (header.entry_point as usize) - lowest_vaddr
        } else {
            0
        };

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

        let user_entry = proc.root_vmar.base + lowest_vaddr + entry_offset;

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
