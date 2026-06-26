pub mod console;
pub mod cpu;
pub mod mmu;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "riscv64")]
pub mod riscv64;
#[cfg(target_arch = "x86_64")]
pub mod x86_64;

pub fn early_init() {
    #[cfg(target_arch = "aarch64")]
    aarch64::early_init();
    #[cfg(target_arch = "riscv64")]
    riscv64::early_init();
    #[cfg(target_arch = "x86_64")]
    x86_64::early_init();
}

pub fn console_putchar(c: u8) {
    crate::drivers::uart::putchar(c);
}

pub fn console_getchar() -> Option<u8> {
    crate::drivers::uart::getchar()
}