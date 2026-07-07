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
