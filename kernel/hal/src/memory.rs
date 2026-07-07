use shared::status::Result;

#[derive(Debug, Clone, Copy)]
pub struct PhysAddr(usize);

impl PhysAddr {
    pub fn new(addr: usize) -> Self { PhysAddr(addr) }
    pub fn as_usize(&self) -> usize { self.0 }
}

#[derive(Debug, Clone, Copy)]
pub struct PhysMemRegion {
    pub base: usize,
    pub size: usize,
    pub region_type: PhysRegionType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysRegionType {
    Memory,
    Device,
    Reserved,
}

pub trait PhysicalMemory: Send + Sync {
    fn total_size() -> usize;
    fn alloc_page(&mut self) -> Result<PhysAddr>;
    fn free_page(&mut self, addr: PhysAddr) -> Result<()>;
    fn query(addr: PhysAddr) -> Result<PhysMemRegion>;
}
