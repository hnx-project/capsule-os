pub struct Ns16550 {
    base: usize,
}

impl Ns16550 {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    fn read_u8(&self, offset: u16) -> u8 {
        unsafe { core::ptr::read_volatile((self.base + offset as usize) as *const u8) }
    }

    fn write_u8(&self, offset: u16, val: u8) {
        unsafe { core::ptr::write_volatile((self.base + offset as usize) as *mut u8, val) }
    }

    pub fn init(&self) {
        self.write_u8(1, 0x00);
        self.write_u8(2, 0xC7);
    }

    pub fn putchar(&self, c: u8) {
        while (self.read_u8(5) & 0x20) == 0 {}
        self.write_u8(0, c);
    }

    pub fn getchar(&self) -> Option<u8> {
        while (self.read_u8(5) & 0x01) == 0 {}
        let c = self.read_u8(0);
        if c == 0 { None } else { Some(c) }
    }
}
