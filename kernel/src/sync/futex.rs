use shared::status::Status;

pub struct Futex;

impl Futex {
    pub fn new() -> Self { Futex }
    pub fn wait(&self, _addr: usize, _expected: u32) -> Status { Status::Ok }
    pub fn wake(&self, _addr: usize) -> Status { Status::Ok }
    pub fn requeue(&self, _addr: usize, _count: u32, _new_addr: usize) -> Status { Status::Ok }
}
