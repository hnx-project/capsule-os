pub mod alloc;
pub mod debug;

pub fn init() { alloc::init(); }

pub fn debug_print(_msg: &str) {}
