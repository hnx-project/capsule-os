pub mod pl011;
pub mod ns16550;

pub use pl011::Pl011;
pub use ns16550::Ns16550;

pub enum ActiveConsole {
    Pl011(Pl011),
    Ns16550(Ns16550),
    None,
}

static mut CONSOLE: ActiveConsole = ActiveConsole::None;

pub fn init_pl011(base: usize) {
    let driver = Pl011::new(base);
    driver.init();
    unsafe {
        CONSOLE = ActiveConsole::Pl011(driver);
    }
}

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
