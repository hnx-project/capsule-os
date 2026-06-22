#![allow(dead_code)]

use shared::status::Result;

pub struct AArch64PageTable;

impl AArch64PageTable {
    pub fn new() -> Result<AArch64PageTable> {
        Ok(AArch64PageTable)
    }
}

#[derive(Clone, Copy)]
pub struct AArch64PageFlags(u64);

impl AArch64PageFlags {
    pub fn read() -> Self { Self(1 << 6) }
    pub fn write() -> Self { Self(1 << 7) }
    pub fn execute() -> Self { Self(1 << 8) }
    pub fn user() -> Self { Self(1 << 5) }
    pub fn kernel() -> Self { Self(0) }
    pub fn device() -> Self { Self(1 << 2) }
    pub fn none() -> Self { Self(0) }
    pub fn with_read(self) -> Self { Self(self.0 | (1 << 6)) }
    pub fn with_write(self) -> Self { Self(self.0 | (1 << 7)) }
    pub fn with_execute(self) -> Self { Self(self.0 | (1 << 8)) }
}

pub struct AArch64AddressSpace {
    table: AArch64PageTable,
}

impl AArch64AddressSpace {
    pub fn new(_base: usize, _size: usize) -> Result<AArch64AddressSpace> {
        Ok(AArch64AddressSpace { table: AArch64PageTable })
    }
    pub fn activate(&self) {}
    pub fn table(&self) -> &AArch64PageTable { &self.table }
}

pub struct AArch64Mmu;

impl AArch64Mmu {
    pub fn new() -> Result<AArch64Mmu> {
        Ok(AArch64Mmu)
    }
    pub fn enable(&self, _aspace: &AArch64AddressSpace) {}
    pub fn disable(&self) {}
    pub fn is_enabled() -> bool { false }
}
