use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::Result;
use crate::arch::mmu_facade::pa_to_kernel_va;
use crate::arch::mmu::PAGE_SIZE;
use crate::arch::phys::{self, PhysAddr};
use crate::arch::ArchContext;

pub const KERNEL_STACK_PAGES: usize = 16;
pub const KERNEL_STACK_SIZE: usize = KERNEL_STACK_PAGES * PAGE_SIZE;

pub const DEFAULT_TIME_SLICE: usize = 5;

static THREAD_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Realtime = 0,
    High = 1,
    Normal = 2,
    Low = 3,
    Idle = 4,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

pub type ThreadContext = <crate::arch::CurrentArch as crate::arch::ArchHardware>::Context;

#[derive(Debug)]
pub struct Thread {
    pub id: usize,
    pub name: &'static str,
    pub state: ThreadState,
    pub priority: Priority,
    pub time_slice: usize,
    pub remaining_ticks: usize,
    pub process_id: u64,
    pub entry: usize,
    pub kernel_stack_base_pa: PhysAddr,
    pub kernel_stack_size: usize,
    pub kernel_sp: usize,
    pub context: ThreadContext,

    pub ipc_buf_ptr: usize,
    pub ipc_buf_len: usize,
    pub ipc_actual_len: usize,
    pub ipc_transfer_handles: [Option<u32>; 4],
    pub handle_table: *const crate::object::handle_table::HandleTable,
    pub ipc_transfer_slots: [Option<(crate::object::handle_table::KernelObject, u32)>; 2],
    pub port_packet_slot: Option<crate::ipc::port::PortPacket>,
    pub sleep_until: Option<u64>,
    /// When the thread is in `Blocked` waiting for a child process
    /// to exit, this records the pid filter passed to `wait4`.
    /// `0` means "any direct child".  Used by the process
    /// reaper to figure out which threads to wake on exit.
    pub wait_child_pid: i64,

    /// SMP-only: physical CPU slot that currently owns this thread.
    /// `None` when the thread is free to be scheduled; `Some(c)`
    /// while the thread is running on CPU slot `c`.  Updated
    /// atomically inside the scheduler lock, never read outside of
    /// the lock except by the scheduler itself.
    pub owner_core: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Initial,
    Ready,
    Running,
    Blocked,
    Sleeping,
    Dead,
}

#[no_mangle]
pub extern "C" fn thread_bootstrap() -> ! {
    // 1. Enable interrupts globally via architecture trap handle
    #[cfg(target_arch = "aarch64")]
    crate::arch::aarch64::trap::enable_irqs();

    // 2. Fetch the actual entry point and execute it
    unsafe {
        #[cfg(target_arch = "aarch64")]
        core::arch::asm!(
            "mov x0, x19",
            "br x19",
            options(noreturn)
        );
        #[cfg(not(target_arch = "aarch64"))]
        {
            loop {}
        }
    }
}

impl Thread {
    /// Allocate a fresh, independent kernel stack of
    /// `KERNEL_STACK_PAGES` 4 KiB pages.  Returns
    /// `(base_pa, va_top)` so callers can place a context at the
    /// top of the new stack or copy an existing kernel stack
    /// into it.
    ///
    /// Used by `sys_fork` so the child has its own kernel-stack
    /// pages; the parent and child must never share a stack
    /// because either may take an EL1 exception (timer IRQ,
    /// syscall, page fault) at any moment and would corrupt the
    /// other's frame area otherwise.
    pub fn alloc_independent_kstack() -> Result<(PhysAddr, usize)> {
        let base_pa = phys::alloc_kstack_page()?;
        let mut last_ok = true;
        for _ in 1..KERNEL_STACK_PAGES {
            if phys::alloc_kstack_page().is_err() {
                last_ok = false;
                break;
            }
        }
        if !last_ok {
            // Partial allocation: roll back whatever we managed to
            // grab.  Avoids leaking kernel-stack pages on the OOM
            // path.
            let mut p = base_pa.as_usize();
            for _ in 0..KERNEL_STACK_PAGES {
                phys::free_page(PhysAddr::new(p));
                p += PAGE_SIZE;
            }
            return Err(shared::status::Status::NoMemory);
        }
        let base_va = pa_to_kernel_va(base_pa.as_usize());
        let va_top = base_va + KERNEL_STACK_SIZE;
        Ok((base_pa, va_top))
    }

    /// Free every kernel-stack page backing this Thread.  Called
    /// from `Process::drop` when a thread is finally reclaimed.
    pub fn free_kstack(&self) {
        if self.kernel_stack_base_pa.as_usize() == 0
            || self.kernel_stack_size == 0
        {
            return;
        }
        let mut p = self.kernel_stack_base_pa.as_usize();
        let end = p + self.kernel_stack_size;
        while p < end {
            phys::free_page(PhysAddr::new(p));
            p += PAGE_SIZE;
        }
    }

    pub fn new_kernel(name: &'static str, entry: extern "C" fn()) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let ctx = ThreadContext::new_kernel(entry as usize, stack_top);

        Ok(Thread {
            id: THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            state: ThreadState::Initial,
            priority: Priority::Normal,
            time_slice: DEFAULT_TIME_SLICE,
            remaining_ticks: DEFAULT_TIME_SLICE,
            process_id: 0,
            entry: entry as usize,
            kernel_stack_base_pa: stack_pa0,
            kernel_stack_size: KERNEL_STACK_SIZE,
            kernel_sp: stack_top,
            context: ctx,
            ipc_buf_ptr: 0,
            ipc_buf_len: 0,
            ipc_actual_len: 0,
            ipc_transfer_handles: [None; 4],
            handle_table: core::ptr::null(),
            ipc_transfer_slots: [None, None],
            port_packet_slot: None,
            sleep_until: None,
            wait_child_pid: 0,
            owner_core: None,
        })
    }

    pub fn new_user(name: &'static str, entry: usize, stack_top: usize) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let kernel_stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let ctx = ThreadContext::new_user(entry, stack_top, kernel_stack_top);

        Ok(Thread {
            id: THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            state: ThreadState::Initial,
            priority: Priority::Normal,
            time_slice: DEFAULT_TIME_SLICE,
            remaining_ticks: DEFAULT_TIME_SLICE,
            process_id: 0,
            entry,
            kernel_stack_base_pa: stack_pa0,
            kernel_stack_size: KERNEL_STACK_SIZE,
            kernel_sp: kernel_stack_top,
            context: ctx,
            ipc_buf_ptr: 0,
            ipc_buf_len: 0,
            ipc_actual_len: 0,
            ipc_transfer_handles: [None; 4],
            handle_table: core::ptr::null(),
            ipc_transfer_slots: [None, None],
            port_packet_slot: None,
            sleep_until: None,
            wait_child_pid: 0,
            owner_core: None,
        })
    }

    pub fn new_kernel_with_priority(name: &'static str, entry: extern "C" fn(), priority: Priority) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let ctx = ThreadContext::new_kernel(entry as usize, stack_top);

        Ok(Thread {
            id: THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            state: ThreadState::Initial,
            priority,
            time_slice: DEFAULT_TIME_SLICE,
            remaining_ticks: DEFAULT_TIME_SLICE,
            process_id: 0,
            entry: entry as usize,
            kernel_stack_base_pa: stack_pa0,
            kernel_stack_size: KERNEL_STACK_SIZE,
            kernel_sp: stack_top,
            context: ctx,
            ipc_buf_ptr: 0,
            ipc_buf_len: 0,
            ipc_actual_len: 0,
            ipc_transfer_handles: [None; 4],
            handle_table: core::ptr::null(),
            ipc_transfer_slots: [None, None],
            port_packet_slot: None,
            sleep_until: None,
            wait_child_pid: 0,
            owner_core: None,
        })
    }

    pub fn new_user_with_priority(name: &'static str, entry: usize, stack_top: usize, priority: Priority) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let kernel_stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let ctx = ThreadContext::new_user(entry, stack_top, kernel_stack_top);

        Ok(Thread {
            id: THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            state: ThreadState::Initial,
            priority,
            time_slice: DEFAULT_TIME_SLICE,
            remaining_ticks: DEFAULT_TIME_SLICE,
            process_id: 0,
            entry,
            kernel_stack_base_pa: stack_pa0,
            kernel_stack_size: KERNEL_STACK_SIZE,
            kernel_sp: kernel_stack_top,
            context: ctx,
            ipc_buf_ptr: 0,
            ipc_buf_len: 0,
            ipc_actual_len: 0,
            ipc_transfer_handles: [None; 4],
            handle_table: core::ptr::null(),
            ipc_transfer_slots: [None, None],
            port_packet_slot: None,
            sleep_until: None,
            wait_child_pid: 0,
            owner_core: None,
        })
    }

    pub fn reset_time_slice(&mut self) {
        self.remaining_ticks = self.time_slice;
    }

    pub fn decay_priority(&mut self) {
        match self.priority {
            Priority::Realtime => self.priority = Priority::High,
            Priority::High => self.priority = Priority::Normal,
            Priority::Normal => self.priority = Priority::Low,
            Priority::Low => self.priority = Priority::Low,
            Priority::Idle => self.priority = Priority::Idle,
        }
        self.reset_time_slice();
    }
}

/// `user_eret_stub` is the resume target for every user thread.  It loads the
/// stashed user-mode state (sp_el0, elr_el1, spsr_el1, x0..x18) out of the
/// incoming `ThreadContext` pointer (passed in `x1` by `switch_to`) and
/// `eret`s into EL0.  The `ThreadContext` is the destination of the most
/// recent context switch (or the freshly-initialised one for a brand-new
/// thread), so this trampoline gives us a single uniform entry point whether
/// the thread is running for the first time or being resumed after
/// preemption.
#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    r#"
.section .text
.global user_eret_stub
user_eret_stub:
    // Atomic Shield: Mask interrupts at the CPU level (set PSTATE.I)
    // to shield the context loading and page-table transition from preemption.
    // The eret to EL0 will automatically and atomically unmask IRQs via SPSR.
    msr     daifset, #2

    // Save the ThreadContext pointer (was in x1) into a callee-saved reg
    mov x9,  x1

    // First: Load user-state bits and page table to EL0 system registers BEFORE clobbering x0-x18.
    ldr x2, [x9, #256]    // user_sp
    msr sp_el0, x2
    ldr x3, [x9, #264]    // elr
    msr elr_el1, x3
    ldr x4, [x9, #272]    // spsr
    msr spsr_el1, x4

    // Crucial Step 1: Pre-load the kernel stack pointer (sp_el1) from ThreadContext.sp (offset 248)
    // so that any exception, timer interrupt or syscall taking us back from EL0 to EL1 has
    // a valid, clean, dedicated per-thread kernel stack to save TrapFrame on!
    ldr x5, [x9, #248]    // sp (kernel SP)
    mov sp, x5

    // Crucial Step 2: Load the process page table (TTBR0_EL1)
    // inside the trampoline, just before the eret boundary.
    ldr x5, [x9, #288]    // l0_user_pa
    cbz x5, 1f            // if zero, skip TTBR0 load (kernel threads)
    msr ttbr0_el1, x5
    tlbi vmalle1          // Zircon-style TLB & ASID Flash Barrier!
    dsb sy
    isb
1:
    // Invalidate icache at the user entry VA so any stale icache lines
    // from a previous address space mapping are discarded before eret.
    // On QEMU-TCG this prevents VIPT aliasing issues where the same VA
    // in a different process's context could cause wrong instruction bytes.
    ic ivau, x3
    dsb sy
    isb

    // Second: Now restore x0..x18 from ThreadContext.x.
    ldp x0,  x1,  [x9, #0]
    ldp x2,  x3,  [x9, #16]
    ldp x4,  x5,  [x9, #32]
    ldp x6,  x7,  [x9, #48]
    ldp x10, x11, [x9, #80]
    ldp x12, x13, [x9, #96]
    ldp x14, x15, [x9, #112]
    ldp x16, x17, [x9, #128]
    ldr x18,      [x9, #144]
    // x8 was clobbered in the stp above; load it explicitly.
    ldr x8,       [x9, #64]

    eret
"#
);

extern "C" {
    fn user_eret_stub() -> !;
}

/// Return the address of `user_eret_stub`.  `sys_fork` uses
/// this to reset the child's `r[11]` (x30) so the first
/// `switch_to` after a fork lands at the right trampoline
/// rather than wherever the parent's last `bl` left the link
/// register (the `syscall!` macro clobbers x30).
pub fn user_eret_stub_addr() -> usize {
    user_eret_stub as usize
}
