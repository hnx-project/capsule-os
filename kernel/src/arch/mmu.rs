use shared::status::Result;
use crate::hal::mmu::PageFlags;

pub struct AArch64PageTable;

impl PageFlags for AArch64PageTable {
    type FLAGS = AArch64PageFlags;
    fn new() -> Result<()> { Ok(()) }
    fn map(&mut self, _va: usize, _pa: usize, _flags: Self::FLAGS) -> Result<()> { Ok(()) }
    fn unmap(&mut self, _va: usize) -> Result<()> { Ok(()) }
    fn query(&self, _va: usize) -> Result<(usize, Self::FLAGS)> { Ok((0, AArch64PageFlags::none())) }
}

#[derive(Clone, Copy)]
pub struct AArch64PageFlags(u64);

impl PageFlags for AArch64PageFlags {
    fn read() -> Self { Self(1 << 6) }
    fn write() -> Self { Self(1 << 7) }
    fn execute() -> Self { Self(1 << 8) }
    fn user() -> Self { Self(1 << 5) }
    fn kernel() -> Self { Self(0) }
    fn device() -> Self { Self(1 << 2) }
    fn none() -> Self { Self(0) }
    fn with_read(self) -> Self { Self(self.0 | (1 << 6)) }
    fn with_write(self) -> Self { Self(self.0 | (1 << 7)) }
    fn with_execute(self) -> Self { Self(self.0 | (1 << 8)) }
}

pub struct AArch64AddressSpace {
    table: AArch64PageTable,
}

impl crate::hal::mmu::AddressSpace for AArch64AddressSpace {
    type PT = AArch64PageTable;
    fn new(_base: usize, _size: usize) -> Result<()> { Ok(()) }
    fn activate(&self) {}
    fn table(&self) -> &Self::PT { &self.table }
}

pub struct AArch64Mmu;

impl crate::hal::mmu::Mmu for AArch64Mmu {
    type AS = AArch64AddressSpace;
    type PT = AArch64PageTable;
    fn new() -> Result<()> { Ok(()) }
    fn enable(&self, _aspace: &Self::AS) {}
    fn disable(&self) {}
    fn is_enabled() -> bool { false }
}
