use shared::status::Status;

pub fn init() {}
pub fn alloc_page() -> Status { Status::NoMemory }
pub fn free_page(_addr: PhysAddr) -> Status { Status::Ok }

#[derive(Debug, Clone, Copy)]
pub struct PhysAddr(usize);
impl PhysAddr { pub fn new(addr: usize) -> Self { PhysAddr(addr) } pub fn as_usize(&self) -> usize { self.0 } }
