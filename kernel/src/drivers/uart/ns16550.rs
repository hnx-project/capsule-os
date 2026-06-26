pub struct Ns16550 {
    base: usize,
}

impl Ns16550 {
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    #[inline(always)]
    fn rbr(&self) -> *mut u8 {
        self.base as *mut u8
    }

    #[inline(always)]
    fn thr(&self) -> *mut u8 {
        self.base as *mut u8
    }

    #[inline(always)]
    fn lsr(&self) -> *const u8 {
        (self.base + 5) as *const u8
    }

    pub fn init(&self) {
        unsafe {
            let ier = (self.base + 1) as *mut u8;
            let fcr = (self.base + 2) as *mut u8;

            // Disable interrupts
            core::ptr::write_volatile(ier, 0x00);
            // Enable FIFO, clear RX/TX FIFO, 14-byte threshold
            core::ptr::write_volatile(fcr, 0xC7);
        }
    }

    pub fn putchar(&self, c: u8) {
        while unsafe { (self.lsr().read_volatile() & 0x20) == 0 } {}
        unsafe {
            self.thr().write_volatile(c);
        }
    }

    pub fn getchar(&self) -> Option<u8> {
        while unsafe { (self.lsr().read_volatile() & 0x01) == 0 } {}
        let c = unsafe { self.rbr().read_volatile() };
        if c == 0 {
            None
        } else {
            Some(c)
        }
    }
}
