pub const ENOSYS: i32 = 38;

#[no_mangle]
pub static mut errno: i32 = 0;

pub fn set_errno_and_fail(err: i32) -> i32 {
    unsafe {
        errno = err;
    }
    -1
}

#[no_mangle]
pub extern "C" fn fork() -> i32 {
    set_errno_and_fail(ENOSYS)
}

#[no_mangle]
pub extern "C" fn execv(_path: *const u8, _argv: *const *const u8) -> i32 {
    set_errno_and_fail(ENOSYS)
}

#[no_mangle]
pub extern "C" fn chmod(_path: *const u8, _mode: u32) -> i32 {
    set_errno_and_fail(ENOSYS)
}

#[no_mangle]
pub extern "C" fn symlink(_target: *const u8, _linkpath: *const u8) -> i32 {
    set_errno_and_fail(ENOSYS)
}



#[no_mangle]
pub extern "C" fn ioctl(_fd: i32, _request: usize, _arg: *mut u8) -> i32 {
    set_errno_and_fail(ENOSYS)
}
