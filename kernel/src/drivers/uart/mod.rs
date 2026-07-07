pub mod pl011;
pub mod ns16550;

pub use pl011::Pl011;
pub use ns16550::Ns16550;

/// Hard-coded raw byte sink used by trap / panic paths that need
/// to emit a string *before* the FDT-driven UART has been
/// initialised.  Always writes to the QEMU `virt` PL011 at
/// 0x0900_0000 (AArch64 bring-up).
pub fn putchar_pl011_raw(c: u8) {
    use core::ptr::{read_volatile, write_volatile};
    unsafe {
        let base = 0x0900_0000usize as *mut u8;
        // Wait for TX ready (PL011 FR[5] == TXFF bit, 0 == ready).
        while (read_volatile(base.add(0x18)) & (1 << 5)) != 0 {}
        write_volatile(base, c);
    }
}

pub enum ActiveConsole {
    Pl011(Pl011),
    Ns16550(Ns16550),
    None,
}

static mut CONSOLE: ActiveConsole = ActiveConsole::None;

/// Initialise the PL011 driver.
///
/// `base` is the MMIO base address parsed from the DTB. Drivers are
/// intentionally MMU-agnostic: the address is treated as a physical
/// address and accessed directly. Identity mapping in the kernel page
/// tables keeps this working after the MMU is enabled.
pub fn init_pl011(base: usize) {
    let driver = Pl011::new(base);
    driver.init();
    unsafe {
        CONSOLE = ActiveConsole::Pl011(driver);
    }
}

/// Initialise the NS16550 driver. See [`init_pl011`] for the `base` semantics.
pub fn init_ns16550(base: usize) {
    let driver = Ns16550::new(base);
    driver.init();
    unsafe {
        CONSOLE = ActiveConsole::Ns16550(driver);
    }
}

pub fn putchar(c: u8) {
    unsafe {
        match &*core::ptr::addr_of!(CONSOLE) {
            ActiveConsole::Pl011(drv) => drv.putchar(c),
            ActiveConsole::Ns16550(drv) => drv.putchar(c),
            ActiveConsole::None => {}
        }
    }
}

pub fn getchar() -> Option<u8> {
    unsafe {
        match &*core::ptr::addr_of!(CONSOLE) {
            ActiveConsole::Pl011(drv) => drv.getchar(),
            ActiveConsole::Ns16550(drv) => drv.getchar(),
            ActiveConsole::None => None,
        }
    }
}
