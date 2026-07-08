/// Virtual physical memory manager for the OHLINK Runner.
/// Emulates 4GB address space, intercepting writes to QEMU Virt UART (0x09000000).

pub struct Mmu {
    // We map segments dynamically using flat buffers or pages.
    // For a lightweight runner, we provide a large virtual vector for RAM.
    // Standard RAM starts at 0x40000000 (1GB offset) in QEMU Virt.
    // We support up to 16MB of virtual RAM.
    ram_base: u64,
    ram: Vec<u8>,
}

impl Mmu {
    pub const UART_BASE: u64 = 0x09000000;
    pub const RAM_DEFAULT_BASE: u64 = 0x40000000;
    pub const RAM_SIZE: usize = 16 * 1024 * 1024; // 16 MB

    pub fn new() -> Self {
        Self {
            ram_base: Self::RAM_DEFAULT_BASE,
            ram: vec![0u8; Self::RAM_SIZE],
        }
    }

    pub fn ram_base(&self) -> u64 {
        self.ram_base
    }

    /// Read an 8-bit byte from virtual memory
    pub fn read8(&self, addr: u64) -> u8 {
        if addr >= self.ram_base && addr < self.ram_base + Self::RAM_SIZE as u64 {
            let offset = (addr - self.ram_base) as usize;
            self.ram[offset]
        } else {
            0
        }
    }

    /// Read a 32-bit word (Little Endian)
    pub fn read32(&self, addr: u64) -> u32 {
        let b0 = self.read8(addr) as u32;
        let b1 = self.read8(addr + 1) as u32;
        let b2 = self.read8(addr + 2) as u32;
        let b3 = self.read8(addr + 3) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    /// Read a 64-bit doubleword (Little Endian)
    pub fn read64(&self, addr: u64) -> u64 {
        let w0 = self.read32(addr) as u64;
        let w1 = self.read32(addr + 4) as u64;
        w0 | (w1 << 32)
    }

    /// Write an 8-bit byte to virtual memory, intercepting UART writes
    pub fn write8(&mut self, addr: u64, val: u8) {
        if addr == Self::UART_BASE {
            // UART Output Intercept! Print the byte straight to macOS stdout
            let char_val = val as char;
            print!("{}", char_val);
            use std::io::Write;
            let _ = std::io::stdout().flush();
        } else if addr >= self.ram_base && addr < self.ram_base + Self::RAM_SIZE as u64 {
            let offset = (addr - self.ram_base) as usize;
            self.ram[offset] = val;
        }
    }

    /// Write a 32-bit word to virtual memory (Little Endian)
    pub fn write32(&mut self, addr: u64, val: u32) {
        self.write8(addr, (val & 0xFF) as u8);
        self.write8(addr + 1, ((val >> 8) & 0xFF) as u8);
        self.write8(addr + 2, ((val >> 16) & 0xFF) as u8);
        self.write8(addr + 3, ((val >> 24) & 0xFF) as u8);
    }

    /// Write a 64-bit doubleword to virtual memory (Little Endian)
    pub fn write64(&mut self, addr: u64, val: u64) {
        self.write32(addr, (val & 0xFFFFFFFF) as u32);
        self.write32(addr + 4, (val >> 32) as u32);
    }

    /// Load segment data into virtual memory RAM at specific physical base offset
    pub fn load_segment(&mut self, start_addr: u64, data: &[u8]) {
        for (i, &byte) in data.iter().enumerate() {
            self.write8(start_addr + i as u64, byte);
        }
    }
}
