pub mod process;
pub mod memory;
pub mod ipc;

pub fn sys_write(_channel: usize, _ptr: usize, _len: usize) -> usize { 0 }
