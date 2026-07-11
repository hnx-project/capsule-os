pub mod console;
pub mod mmu;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "riscv64")]
pub mod riscv64;

pub fn early_init() {
    #[cfg(target_arch = "aarch64")]
    aarch64::early_init();
    #[cfg(target_arch = "riscv64")]
    riscv64::early_init();
}

pub fn console_putchar(c: u8) {
    crate::drivers::uart::putchar(c);
}

pub fn console_getchar() -> Option<u8> {
    crate::drivers::uart::getchar()
}

/// Bulk-bytes sink used by `aarch64::trap::panic_unhandled` and
/// any other low-level panic path.  Goes straight to the UART
/// without the `'\n' -> '\r'` rewrite that `console_putchar`
/// performs (so panic dumps don't garble binary markers).
pub fn console_putbytes(s: &[u8]) {
    for &c in s {
        console_putchar(c);
    }
}

/// Architecture-specific trap and interrupt helpers.
#[cfg(target_arch = "aarch64")]
pub use aarch64::trap;
#[cfg(target_arch = "riscv64")]
pub use riscv64::trap;

#[cfg(target_arch = "aarch64")]
pub use aarch64::mmu::translate_user_va;
#[cfg(target_arch = "riscv64")]
pub use riscv64::mmu::translate_user_va;