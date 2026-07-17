use core::sync::atomic::{AtomicUsize, Ordering};
use shared::status::Result;
use crate::mm::mmu::pa_to_kernel_va;
use crate::mm::mmu::PAGE_SIZE;
use crate::mm::phys::{self, PhysAddr};

pub const KERNEL_STACK_PAGES: usize = 4;
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

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy)]
pub struct ThreadContext {
    pub x: [u64; 19],   // x0..x18 caller-saved (persisted across context switches)
    pub r: [u64; 12],   // x19..x30 callee-saved
    pub sp: u64,        // kernel SP (sp_el1)
    pub user_sp: u64,   // user SP (sp_el0)
    pub elr: u64,       // user PC to eret to on resume
    pub spsr: u64,      // saved PSTATE (saved copy of user SPSR on trap)
    pub process_id: u64,          // PID, for TTBR0_EL1 in assembly
    pub l0_user_pa: u64,          // L0 PA, for TTBR0_EL1 in assembly
    pub page_table_gen: u64,      // generation stamp from PageTableTree (stale-handle detection)
}

const _ASSERT_LAYOUT: () = {
    if core::mem::size_of::<[u64; 19]>() != 152 { panic!("x size mismatch"); }
    if core::mem::size_of::<[u64; 12]>() != 96 { panic!("r size mismatch"); }
    if core::mem::offset_of!(ThreadContext, sp) != 248 { panic!("sp offset mismatch"); }
    if core::mem::offset_of!(ThreadContext, user_sp) != 256 { panic!("user_sp offset mismatch"); }
    if core::mem::offset_of!(ThreadContext, elr) != 264 { panic!("elr offset mismatch"); }
    if core::mem::offset_of!(ThreadContext, spsr) != 272 { panic!("spsr offset mismatch"); }
    if core::mem::offset_of!(ThreadContext, process_id) != 280 { panic!("process_id offset mismatch"); }
    if core::mem::offset_of!(ThreadContext, l0_user_pa) != 288 { panic!("l0_user_pa offset mismatch"); }
};

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
    // 1. Enable interrupts globally
    #[cfg(target_arch = "aarch64")]
    crate::arch::aarch64::trap::enable_irqs();
    #[cfg(target_arch = "riscv64")]
    crate::arch::riscv64::trap::enable_irqs();

    // 2. Fetch the actual entry point and execute it
    unsafe {
        #[cfg(target_arch = "aarch64")]
        core::arch::asm!(
            "mov x0, x19",
            "br x19",
            options(noreturn)
        );
        #[cfg(target_arch = "riscv64")]
        core::arch::asm!(
            "mv a0, s0",
            "jr s0",
            options(noreturn)
        );
    }
}

impl Thread {
    pub fn new_kernel(name: &'static str, entry: extern "C" fn()) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let mut ctx = ThreadContext::default();
        ctx.sp = stack_top as u64;
        ctx.elr = thread_bootstrap as usize as u64; // Set initial target PC to bootstrap
        ctx.r[0] = entry as usize as u64;            // Save actual entry point in r[0] (x19 or s0)

        #[cfg(target_arch = "aarch64")]
        {
            ctx.spsr = 0x005;  // EL1h, IRQs unmasked
            ctx.r[11] = thread_bootstrap as usize as u64; // Set LR (x30) to bootstrap
        }
        #[cfg(target_arch = "riscv64")]
        {
            ctx.spsr = 0x102;  // S-Mode, SIE=1, SPP=1
        }

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
        })
    }

    pub fn new_user(name: &'static str, entry: usize, stack_top: usize) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let kernel_stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let mut ctx = ThreadContext::default();
        // Force high-half mapping virtual address for kernel stack (sp_el1)
        let high_kernel_stack_top = if kernel_stack_top < 0xffff_8000_0000_0000usize {
            kernel_stack_top | 0xffff_8000_0000_0000usize
        } else {
            kernel_stack_top
        };
        ctx.sp = high_kernel_stack_top as u64; // Kernel stack for interrupts
        ctx.user_sp = stack_top as u64; // initial user-mode SP (sp_el0)
        ctx.elr = entry as u64; // user entry point - first switch will eret to here
        // SPSR M[3:0] = 0b0000 (EL0t) so eret drops into AArch64 user mode.
        // Also enable IRQs (clear mask bits: F=0, I=0, A=0, D=0)
        ctx.spsr = 0x000;
        ctx.r[0] = entry as u64;
        ctx.r[1] = stack_top as u64;

        #[cfg(target_arch = "aarch64")]
        {
            ctx.r[11] = user_eret_stub as usize as u64;
        }

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
        })
    }

    pub fn new_kernel_with_priority(name: &'static str, entry: extern "C" fn(), priority: Priority) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let mut ctx = ThreadContext::default();
        ctx.sp = stack_top as u64;
        ctx.elr = thread_bootstrap as usize as u64;
        ctx.r[0] = entry as usize as u64;

        #[cfg(target_arch = "aarch64")]
        {
            ctx.spsr = 0x3c0; // EL0t, all interrupts (D, A, I, F) masked initially to prevent premature interrupt traps during user startup bootstrap!
        }
        #[cfg(target_arch = "riscv64")]
        {
            ctx.spsr = 0x102;
        }

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
        })
    }

    pub fn new_user_with_priority(name: &'static str, entry: usize, stack_top: usize, priority: Priority) -> Result<Self> {
        let stack_pa0 = phys::alloc_kstack_page()?;
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = phys::alloc_kstack_page()?;
        }
        let kernel_stack_va = pa_to_kernel_va(stack_pa0.as_usize());
        let kernel_stack_top = kernel_stack_va + KERNEL_STACK_SIZE;

        let mut ctx = ThreadContext::default();
        // Force high-half mapping virtual address for kernel stack (sp_el1)
        let high_kernel_stack_top = if kernel_stack_top < 0xffff_8000_0000_0000usize {
            kernel_stack_top | 0xffff_8000_0000_0000usize
        } else {
            kernel_stack_top
        };
        ctx.sp = high_kernel_stack_top as u64;
        ctx.user_sp = stack_top as u64;
        ctx.elr = entry as u64;
        ctx.spsr = 0x000;
        ctx.r[0] = entry as u64;
        ctx.r[1] = stack_top as u64;

        #[cfg(target_arch = "aarch64")]
        {
            ctx.r[11] = user_eret_stub as usize as u64;
        }

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
