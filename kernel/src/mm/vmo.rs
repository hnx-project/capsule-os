use shared::status::{Result, Status};
use core::sync::atomic::{AtomicUsize, Ordering};

static VMO_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug)]
pub struct Vmo {
    pub id: u64,
    pub size: usize,
}

impl Vmo {
    pub fn new() -> Result<Self> {
        Ok(Vmo { id: VMO_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u64, size: 0 })
    }
    pub fn create_with_size(size: usize) -> Result<Self> {
        Ok(Vmo { id: VMO_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u64, size })
    }
    pub fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize> { Ok(0) }
    pub fn write(&mut self, _offset: usize, _buf: &[u8]) -> Result<usize> { Ok(0) }
    pub fn get_size(&self) -> usize { self.size }
    pub fn set_size(&mut self, size: usize) -> Result<()> { self.size = size; Ok(()) }
}
