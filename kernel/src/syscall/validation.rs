use shared::status::Result;

pub fn validate_pointer<T>(_ptr: *const T) -> Result<()> { Ok(()) }
pub fn validate_mut_pointer<T>(_ptr: *mut T) -> Result<()> { Ok(()) }
pub fn validate_buffer(_ptr: usize, _len: usize) -> Result<()> { Ok(()) }
pub fn validate_handle(_handle: u32) -> Result<()> { Ok(()) }
