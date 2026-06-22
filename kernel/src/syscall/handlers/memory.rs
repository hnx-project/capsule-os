use shared::status::Status;

pub fn sys_vmo_create(_size: usize) -> usize { Status::Ok.to_raw() }
pub fn sys_vmo_read(_handle: usize, _ptr: usize, _offset: usize, _len: usize) -> usize { Status::Ok.to_raw() }
pub fn sys_vmo_write(_handle: usize, _ptr: usize, _offset: usize, _len: usize) -> usize { Status::Ok.to_raw() }
