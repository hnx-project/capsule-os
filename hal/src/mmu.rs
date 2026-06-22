#![allow(dead_code)]

use shared::status::Result;

pub trait PageFlags: Clone + Copy {
    fn read() -> Self;
    fn write() -> Self;
    fn execute() -> Self;
    fn user() -> Self;
    fn kernel() -> Self;
    fn device() -> Self;
    fn none() -> Self;
    fn with_read(self) -> Self;
    fn with_write(self) -> Self;
    fn with_execute(self) -> Self;
}

pub trait AddressSpace: Send + Sync {
    type PT: PageTable;
    fn new(_base: usize, _size: usize) -> Result<()>;
    fn activate(&self);
    fn table(&self) -> &Self::PT;
}

pub trait PageTable: Send + Sync + 'static {
    type FLAGS: PageFlags;
    fn new() -> Result<()>;
    fn map(&mut self, _va: usize, _pa: usize, _flags: Self::FLAGS) -> Result<()>;
    fn unmap(&mut self, _va: usize) -> Result<()>;
    fn query(&self, _va: usize) -> Result<(usize, Self::FLAGS)>;
}

pub trait Mmu: Send + Sync {
    type AS: AddressSpace;
    type PT: PageTable;
    fn new() -> Result<()>;
    fn enable(&self, _aspace: &Self::AS);
    fn disable(&self);
    fn is_enabled() -> bool;
}
