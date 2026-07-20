pub struct Pl011 {
    base: usize,
}

impl Pl011 {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    #[inline(always)]
    fn dr(&self) -> *mut u8 {
        self.base as *mut u8
    }

    #[inline(always)]
    fn fr(&self) -> *const u8 {
        (self.base + 0x18) as *const u8
    }

    #[inline(always)]
    fn reg(&self, offset: usize) -> *mut u32 {
        (self.base + offset) as *mut u32
    }

    pub fn init(&self) {
        unsafe {
            self.reg(0x30).write_volatile(0);          // Disable
            self.reg(0x30).write_volatile(0x301);      // Enable, RX, TX
            self.reg(0x24).write_volatile(13);         // Integer divisor
            self.reg(0x28).write_volatile(2);          // Fractional divisor
            self.reg(0x2c).write_volatile(0x70);       // 8-bit, FIFO
            self.reg(0x2c).write_volatile(0x71);       // 8-bit, FIFO, enable
            self.reg(0x30).write_volatile(0x301);      // Re-enable
        }
    }

    pub fn putchar(&self, c: u8) {
        while unsafe { (self.fr().read_volatile() & (1 << 5)) != 0 } {}
        unsafe {
            self.dr().write_volatile(c);
        }
    }

    /// Same as `putchar` but writes the byte regardless of whether
    /// `c` is `\n` (the trap path uses this to emit raw `\n`).
    pub fn putchar_raw(&self, c: u8) {
        while unsafe { (self.fr().read_volatile() & (1 << 5)) != 0 } {}
        unsafe { self.dr().write_volatile(c) }
    }

    pub fn getchar(&self) -> Option<u8> {
        while unsafe { (self.fr().read_volatile() & (1 << 4)) != 0 } {}
        let c = unsafe { self.dr().read_volatile() };
        if c == 0 {
            None
        } else {
            Some(c)
        }
    }
}
