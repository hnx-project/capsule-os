const UART0_BASE: usize = 0x09000000;
const UART0_DR: *mut u8 = UART0_BASE as *mut u8;
const UART0_FR: *const u8 = (UART0_BASE + 0x18) as *const u8;

pub fn early_init() {
}

pub fn console_putchar(c: u8) {
    while unsafe { (UART0_FR.read_volatile() & (1 << 5)) != 0 } {}
    unsafe { UART0_DR.write_volatile(c); }
}

pub fn console_getchar() -> Option<u8> {
    while unsafe { (UART0_FR.read_volatile() & (1 << 4)) != 0 } {}
    let c = unsafe { UART0_DR.read_volatile() };
    if c == 0 { None } else { Some(c) }
}
