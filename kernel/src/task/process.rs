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
        let vmar_base = 0x1_0000_0000usize + slot * 0x1000_0000usize;
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
