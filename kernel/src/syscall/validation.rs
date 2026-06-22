use shared::status::Status;

pub fn validate_pointer<T>(_ptr: *const T) -> Status { Status::Ok }
pub fn validate_mut_pointer<T>(_ptr: *mut T) -> Status { Status::Ok }
pub fn validate_buffer(_ptr: usize, _len: usize) -> Status { Status::Ok }
pub fn validate_handle(_handle: u32) -> Status { Status::Ok }
