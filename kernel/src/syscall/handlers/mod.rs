pub mod process;
pub mod memory;
pub mod ipc;

use shared::status::Status;

pub fn sys_write(fd: usize, ptr: usize, len: usize) -> usize {
    if fd == 1 || fd == 2 {
        if ptr == 0 || len == 0 {
            return 0;
        }
        let slice = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
        if let Ok(s) = core::str::from_utf8(slice) {
            for &b in s.as_bytes() {
                crate::arch::console_putchar(b);
            }
        } else {
            for &b in slice {
                crate::arch::console_putchar(b);
            }
        }
        len
    } else {
        Status::NotAllowed.to_raw() as usize
    }
}
