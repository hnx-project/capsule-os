use core::arch::global_asm;

global_asm!(include_str!("boot_asm.S"));

pub fn early_init() {
    // RISC-V CPU level early initialization can be added here
}
