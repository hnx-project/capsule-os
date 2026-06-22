use shared::status::Status;

pub fn sys_channel_create() -> usize { Status::Ok.to_raw() }
pub fn sys_channel_read(_handle: usize, _ptr: usize, _len: usize, _handle_count: usize) -> usize { Status::Ok.to_raw() }
pub fn sys_channel_write(_handle: usize, _ptr: usize, _len: usize, _handle_count: usize) -> usize { Status::Ok.to_raw() }
