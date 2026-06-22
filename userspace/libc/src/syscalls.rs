use shared::status::Status;

#[macro_export]
macro_rules! syscall {
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "svc #0",
                inout("x0") $a0 as usize => ret,
                in("x1") $a1 as usize,
                in("x2") $a2 as usize,
                in("x3") $a3 as usize,
                in("x4") $a4 as usize,
                in("x5") $a5 as usize,
                in("x16") $num,
            );
        }
        ret
    }};
}

pub const SYSCALL_EXIT: u32 = 0;
pub const SYSCALL_WRITE: u32 = 1;
pub const SYSCALL_GET_TID: u32 = 2;
pub const SYSCALL_CHANNEL_CREATE: u32 = 10;
pub const SYSCALL_CHANNEL_READ: u32 = 11;
pub const SYSCALL_CHANNEL_WRITE: u32 = 12;
pub const SYSCALL_VMO_CREATE: u32 = 30;

pub fn exit(code: i32) -> ! {
    syscall!(SYSCALL_EXIT, code as usize, 0, 0, 0, 0, 0);
    loop {}
}

pub fn write(fd: usize, ptr: usize, len: usize) -> usize {
    syscall!(SYSCALL_WRITE, fd, ptr, len, 0, 0, 0)
}

pub fn channel_create() -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_CHANNEL_CREATE, 0, 0, 0, 0, 0, 0);
    if ret == 0 { Err(Status::from_raw(ret as i32)) } else { Ok(ret) }
}
