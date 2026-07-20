use core::arch::{global_asm, asm};
use crate::arch::{ArchContext, ArchHardware, CpuDiagnostics};

global_asm!(include_str!("boot_asm.S"));

// 声明并提取底层的 AArch64 汇编上下文切换与恢复函数
global_asm!(
    r#"
.section .text
.global switch_to
switch_to:
    // Save callee-saved registers x19-x30 from the *current* thread.
    // The caller passes x0 = current ThreadContext*, x1 = next ThreadContext*.
    stp x19, x20, [x0, #152]   // current.r[0..1] -- offset after x[0..18]
    stp x21, x22, [x0, #168]
    stp x23, x24, [x0, #184]
    stp x25, x26, [x0, #200]
    stp x27, x28, [x0, #216]
    stp x29, x30, [x0, #232]
    // Save the kernel's SP (sp_el1).
    mov x2, sp
    str x2, [x0, #248]         // current.sp
    // Save the user-mode SP (sp_el0).
    mrs x3, sp_el0
    str x3, [x0, #256]         // current.user_sp

    // Atomically stash the next context pointer (x1) into platform register x18
    // to safeguard it from any callee-saved ldp registers overwrite traps (e.g. ldp x27, x28)
    // before we branch to user_eret_stub.
    mov x18, x1

    // Load callee-saved registers for the next thread.
    ldp x19, x20, [x18, #152]
    ldp x21, x22, [x18, #168]
    ldp x23, x24, [x18, #184]
    ldp x25, x26, [x18, #200]
    ldp x27, x28, [x18, #216]
    ldr x29,     [x18, #232]   // x29 = next frame pointer
    // x30 = next.r[11] = user_eret_stub.
    ldr x30, [x18, #240]
    // Load the next thread's kernel SP and user SP (sp_el0).
    ldr x2, [x18, #248]
    mov sp, x2
    ldr x4, [x18, #256]
    msr sp_el0, x4

    // Hard-lock: restore the next context pointer atomic address back into x1 from x18
    // immediately prior to issuing the return branch.
    mov x1, x18

    // Jump into the resume trampoline that restores the user-visible
    // register state from the ThreadContext and erets to EL0.
    ret
    "#
);

extern "C" {
    fn switch_to(current: *mut Aarch64Context, next: *const Aarch64Context);
    fn user_eret_stub() -> !;
}

pub mod asid;
pub mod mmu;
pub mod trap;
pub mod page_table;
pub mod phys;
pub mod slab;

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct Aarch64Context {
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

impl Default for Aarch64Context {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

impl ArchContext for Aarch64Context {
    fn new_kernel(entry: usize, stack_top: usize) -> Self {
        let mut ctx = Self::default();
        ctx.sp = stack_top as u64;
        ctx.elr = crate::task::thread::thread_bootstrap as usize as u64; // Set initial target PC to bootstrap
        ctx.r[0] = entry as u64;                                         // Save actual entry point in r[0] (x19 or s0)
        ctx.spsr = 0x005;                                                // EL1h, IRQs unmasked
        ctx.r[11] = crate::task::thread::thread_bootstrap as usize as u64; // Set LR (x30) to bootstrap
        ctx
    }

    fn new_user(entry: usize, stack_top: usize, kernel_stack_top: usize) -> Self {
        let mut ctx = Self::default();
        // Force high-half mapping virtual address for kernel stack (sp_el1)
        let high_kernel_stack_top = if kernel_stack_top < 0xffff_8000_0000_0000usize {
            kernel_stack_top | 0xffff_8000_0000_0000usize
        } else {
            kernel_stack_top
        };
        ctx.sp = high_kernel_stack_top as u64; // Kernel stack for interrupts
        ctx.user_sp = stack_top as u64;        // initial user-mode SP (sp_el0)
        ctx.elr = entry as u64;                // user entry point - first switch will eret to here
        ctx.spsr = 0x000;                      // SPSR M[3:0] = 0b0000 (EL0t) so eret drops into AArch64 user mode.
        ctx.r[0] = entry as u64;
        ctx.r[1] = stack_top as u64;
        ctx.r[11] = user_eret_stub as usize as u64;
        ctx
    }

    fn set_page_table(&mut self, process_id: u64, l0_user_pa: u64) {
        self.process_id = process_id;
        self.l0_user_pa = l0_user_pa;
    }
}

pub struct Aarch64Hardware;

impl ArchHardware for Aarch64Hardware {
    type Context = Aarch64Context;
    type PageTable = page_table::PageTableTree;
    type AddressSpaceId = u16;

    fn alloc_asid() -> Option<Self::AddressSpaceId> {
        asid::alloc()
    }

    fn free_asid(asid: Self::AddressSpaceId) {
        asid::free(asid);
    }

    fn kernel_asid() -> Self::AddressSpaceId {
        asid::ASID_KERNEL
    }

    fn pack_ttbr(l0_pa: u64, asid: Self::AddressSpaceId) -> u64 {
        asid::pack_ttbr(l0_pa, asid)
    }

    fn rollover_generation() -> u32 {
        asid::rollover_generation()
    }

    unsafe fn sync_instruction_cache(kva: usize, len: usize) {
        mmu::sync_instruction_cache(kva, len);
    }

    unsafe fn invalidate_instruction_cache() {
        asm!("ic ialluis", "dsb sy", "isb", options(nostack));
    }

    unsafe fn switch_context(current: *mut Self::Context, next: *const Self::Context) {
        switch_to(current, next);
    }

    unsafe fn wait_for_interrupt() {
        asm!("wfi", options(nomem, nostack));
    }

    unsafe fn wait_for_event() {
        asm!("wfe", options(nomem, nostack));
    }

    unsafe fn flush_tlb() {
        asm!(
            "dsb ish",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nostack)
        );
    }

    unsafe fn restore_user_page_table(old_val: usize, new_l0_pa: usize, new_asid: Self::AddressSpaceId) {
        if old_val != 0 {
            asm!(
                "msr ttbr0_el1, {val}",
                "isb",
                "tlbi vmalle1is",
                "dsb ish",
                "isb",
                val = in(reg) old_val as u64,
                options(nomem, nostack)
            );
        } else {
            mmu::set_ttbr0_el1(new_l0_pa, new_asid);
            Self::flush_tlb();
        }
    }

    unsafe fn clean_cache_range(kva: usize, len: usize) {
        let cache_line_size = 64; // standard default
        let start = kva & !(cache_line_size - 1);
        let end = kva + len;
        let mut ptr = start;
        while ptr < end {
            asm!("dc cvac, {0}", in(reg) ptr, options(nomem, nostack));
            ptr += cache_line_size;
        }
        asm!(
            "dsb ish",
            "isb",
            options(nomem, nostack)
        );
    }

    unsafe fn clean_and_invalidate_cache_range(kva: usize, len: usize) {
        let cache_line_size = 64;
        let start = kva & !(cache_line_size - 1);
        let end = kva + len;
        let mut ptr = start;
        while ptr < end {
            asm!("dc civac, {0}", in(reg) ptr, options(nomem, nostack));
            ptr += cache_line_size;
        }
        asm!(
            "dsb ish",
            "isb",
            options(nomem, nostack)
        );
    }

    unsafe fn invalidate_stack_line(sp_va: usize) {
        asm!(
            "and x9, {sp_va}, #~0xfff",
            "dc ivac, x9",
            "ic ivau, x9",
            "dsb sy",
            "isb",
            sp_va = in(reg) sp_va,
            options(nomem, nostack)
        );
    }

    unsafe fn local_irq_save() -> usize {
        let mut tmp: usize;
        asm!(
            "mrs {tmp}, daif",
            "msr daifset, #0xf",
            tmp = out(reg) tmp,
            options(nomem, preserves_flags),
        );
        tmp
    }

    unsafe fn local_irq_restore(flags: usize) {
        asm!(
            "msr daif, {0}",
            in(reg) flags,
            options(nomem, preserves_flags),
        );
    }

    unsafe fn memory_barrier() {
        asm!("dsb ish", options(nomem, nostack));
    }

    unsafe fn instruction_barrier() {
        asm!("isb", options(nomem, nostack));
    }

    fn get_current_registers() -> (usize, usize) {
        let mut pc: usize;
        let mut sp: usize;
        unsafe {
            asm!("mov {0}, x30", out(reg) pc, options(nomem, nostack));
            asm!("mov {0}, sp", out(reg) sp, options(nomem, nostack));
        }
        (pc, sp)
    }

    fn get_diagnostics() -> CpuDiagnostics {
        let mut spsr: u64;
        let mut elr: u64;
        let mut esr: u64;
        unsafe {
            asm!("mrs {0}, spsr_el1", out(reg) spsr, options(nomem, nostack));
            asm!("mrs {0}, elr_el1", out(reg) elr, options(nomem, nostack));
            asm!("mrs {0}, esr_el1", out(reg) esr, options(nomem, nostack));
        }
        CpuDiagnostics {
            pc: elr as usize,
            sp: 0,
            spsr_or_status: spsr as usize,
            elr_or_epc: elr as usize,
            esr_or_cause: esr as usize,
        }
    }

    fn get_active_page_table() -> usize {
        let mut ttbr0_reg: u64;
        unsafe {
            asm!("mrs {0}, ttbr0_el1", out(reg) ttbr0_reg, options(nomem, nostack));
        }
        (ttbr0_reg & 0x0000_FFFF_FFFF_F000) as usize
    }

    fn get_hardware_ticks() -> u64 {
        let mut v: u64;
        unsafe {
            asm!("mrs {0}, cntpct_el0", out(reg) v, options(nomem, preserves_flags));
        }
        v
    }

    fn set_timer_ticks(ticks: u32) {
        unsafe {
            asm!("msr cntp_tval_el0, {0}", in(reg) ticks as u64, options(nomem, preserves_flags));
        }
    }

    fn enable_timer(frequency: usize) {
        // 配置和使能核心 Timer
        unsafe {
            asm!("msr cntp_ctl_el0, {0}", in(reg) 1u64, options(nomem, preserves_flags));
            let _ = frequency;
        }
    }
}

pub fn early_init() {
    // Phase 3.1 wiring happens later
}

pub fn console_putchar(c: u8) {
    if c == b'\n' {
        crate::drivers::uart::putchar_pl011_raw(b'\r');
    }
    crate::drivers::uart::putchar_pl011_raw(c);
}

pub mod boot {
    extern "C" {
        pub fn rust_boot();
    }
}
