use shared::status::{Result, Status};

#[derive(Debug)]
pub struct Vmar {
    pub base: usize,
    pub size: usize,
}

impl Vmar {
    pub fn new() -> Result<Self> {
        Ok(Vmar { base: 0x10000000, size: 256 * 1024 * 1024 })
    }
    pub fn map(&mut self, _vmo_id: u64, _vmo_offset: usize, _virt_addr: usize, _size: usize, _flags: VmarFlags) -> Result<usize> { Ok(0) }
    pub fn unmap(&mut self, _virt_addr: usize, _size: usize) -> Result<()> { Ok(()) }
    pub fn protect(&mut self, _virt_addr: usize, _size: usize, _flags: VmarFlags) -> Result<()> { Ok(()) }
}

#[derive(Debug, Clone, Copy)]
pub struct VmarFlags(u32);

impl VmarFlags {
    pub const NONE: VmarFlags = VmarFlags(0);
    pub const READ: VmarFlags = VmarFlags(1 << 0);
    pub const WRITE: VmarFlags = VmarFlags(1 << 1);
    pub const EXECUTE: VmarFlags = VmarFlags(1 << 2);
}
