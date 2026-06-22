use shared::status::Result;

pub struct Futex;

impl Futex {
    pub fn new() -> Self { Futex }
    pub fn wait(&self, _addr: usize, _expected: u32) -> Result<()> { Ok(()) }
    pub fn wake(&self, _addr: usize) -> Result<u32> { Ok(0) }
    pub fn requeue(&self, _addr: usize, _count: u32, _new_addr: usize) -> Result<u32> { Ok(0) }
}
