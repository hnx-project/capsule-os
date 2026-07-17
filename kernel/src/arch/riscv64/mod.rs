use core::arch::global_asm;

global_asm!(include_str!("boot_asm.S"));
global_asm!(include_str!("trap_asm.S"));

pub mod mmu;
pub mod trap;

extern "C" {
    fn riscv_trap_vector();
}

pub fn early_init() {
    unsafe {
        // Set stvec to point to riscv_trap_vector in Direct mode (lowest 2 bits are 0)
        core::arch::asm!("csrw stvec, {0}", in(reg) riscv_trap_vector as usize);
    }
}

/// Fallback Context representation for Riscv64 during EL0 boot tests.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct Riscv64Context {
    pub gpr: [u64; 32],
    pub sstatus: u64,
    pub sepc: u64,
    pub process_id: u64,
}

impl crate::arch::ArchContext for Riscv64Context {
    fn new_kernel(_entry: usize, _stack_top: usize) -> Self { Self::default() }
    fn new_user(_entry: usize, _stack_top: usize, _kernel_stack_top: usize) -> Self { Self::default() }
    fn set_page_table(&mut self, _process_id: u64, _l0_user_pa: u64) {}
}

pub struct Riscv64Hardware;

impl crate::arch::ArchHardware for Riscv64Hardware {
    type Context = Riscv64Context;
    unsafe fn switch_context(_current: *mut Self::Context, _next: *const Self::Context) {}
    unsafe fn wait_for_interrupt() { core::arch::asm!("wfi", options(nomem, nostack)); }
    unsafe fn wait_for_event() {}
    unsafe fn flush_tlb() { core::arch::asm!("sfence.vma", options(nomem, nostack)); }
    unsafe fn clean_cache_range(_kva: usize, _len: usize) {}
    unsafe fn clean_and_invalidate_cache_range(_kva: usize, _len: usize) {}
    unsafe fn invalidate_stack_line(_sp_va: usize) {}
    unsafe fn memory_barrier() { core::arch::asm!("fence", options(nomem, nostack)); }
    unsafe fn instruction_barrier() { core::arch::asm!("fence.i", options(nomem, nostack)); }
    fn get_current_registers() -> (usize, usize) { (0, 0) }
    fn get_diagnostics() -> crate::arch::CpuDiagnostics { crate::arch::CpuDiagnostics::default() }
    fn get_active_page_table() -> usize { 0 }
    fn get_hardware_ticks() -> u64 { 0 }
    fn set_timer_ticks(_ticks: u32) {}
    fn enable_timer(_frequency: usize) {}
}
