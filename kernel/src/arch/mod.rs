pub mod console;
pub mod cpu;
pub mod mmu;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "x86_64")]
pub mod x86_64;

pub fn early_init() {
    #[cfg(target_arch = "aarch64")]
    aarch64::early_init();
    #[cfg(target_arch = "x86_64")]
    x86_64::early_init();
}

#[cfg(target_arch = "aarch64")]
pub fn set_uart_base(base: usize) { aarch64::set_uart_base(base); }
#[cfg(not(target_arch = "aarch64"))]
pub fn set_uart_base(_base: usize) {}

#[cfg(target_arch = "aarch64")]
pub fn console_putchar(c: u8) { aarch64::console_putchar(c); }
#[cfg(target_arch = "x86_64")]
pub fn console_putchar(_c: u8) {}

#[cfg(target_arch = "aarch64")]
pub fn console_getchar() -> Option<u8> { aarch64::console_getchar() }
#[cfg(target_arch = "x86_64")]
pub fn console_getchar() -> Option<u8> { None }