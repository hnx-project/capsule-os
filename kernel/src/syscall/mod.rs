pub mod numbers;
pub mod validation;
pub mod handlers;

pub use numbers::*;
pub use validation::*;

use shared::status::Status;

pub fn syscall_dispatch(_syscall_num: u32, _arg0: usize, _arg1: usize, _arg2: usize, _arg3: usize, _arg4: usize, _arg5: usize) -> usize {
    Status::Ok.to_raw()
}
