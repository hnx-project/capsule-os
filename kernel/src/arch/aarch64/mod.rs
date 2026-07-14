use core::arch::global_asm;

global_asm!(include_str!("boot_asm.S"));

pub mod asid;
pub mod mmu;
pub mod trap;

pub fn early_init() {
    // Phase 3.1 wiring happens later: GIC + generic timer are
    // installed by `kernel_main` after the page tables are live
    // (they need to know the physical base of each device, which
    // we get from the FDT).  All `early_init` has to do today is
    // arrange that the vector table is in place -- `boot_asm.S`
    // already did that before we got here, so this is a no-op.
}

/// Raw byte sink used by the panic path in `trap.rs`.  Goes
/// straight to the PL011 MMIO window (PA 0x0900_0000); on AArch64
/// the MMU identity-maps that range via the L1 block, so the
/// write works regardless of translation state.
pub fn console_putbytes(s: &[u8]) {
    for &c in s {
        console_putchar(c);
    }
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