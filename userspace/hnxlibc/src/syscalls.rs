use shared::status::Status;

#[macro_export]
macro_rules! syscall {
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let ret: usize;
        #[cfg(target_arch = "aarch64")]
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
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!(
                "ecall",
                inout("a0") $a0 as usize => ret,
                in("a1") $a1 as usize,
                in("a2") $a2 as usize,
                in("a3") $a3 as usize,
                in("a4") $a4 as usize,
                in("a5") $a5 as usize,
                in("a7") $num,
            );
        }
        ret
    }};
}

pub const SYSCALL_EXIT: u32 = 0;
pub const SYSCALL_WRITE: u32 = 1;
pub const SYSCALL_READ: u32 = 2;
pub const SYSCALL_OPEN: u32 = 3;
pub const SYSCALL_CLOSE: u32 = 4;
pub const SYSCALL_GET_TID: u32 = 5;
pub const SYSCALL_CHANNEL_CREATE: u32 = 10;
pub const SYSCALL_CHANNEL_READ: u32 = 11;
pub const SYSCALL_CHANNEL_WRITE: u32 = 12;
pub const SYSCALL_VMO_CREATE: u32 = 30;
pub const SYSCALL_EXEC: u32 = 110;

pub fn exit(code: i32) -> ! {
    syscall!(SYSCALL_EXIT, code as usize, 0, 0, 0, 0, 0);
    loop {}
}

pub fn write_fd(fd: usize, ptr: usize, len: usize) -> usize {
    syscall!(SYSCALL_WRITE, fd, ptr, len, 0, 0, 0)
}

pub fn channel_create() -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_CHANNEL_CREATE, 0, 0, 0, 0, 0, 0);
    if ret == 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn channel_read(handle: usize, buf: &mut [u8], handles: &mut [u32]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_READ,
        handle,
        buf.as_mut_ptr() as usize,
        buf.len(),
        handles.as_mut_ptr() as usize,
        handles.len(),
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn channel_write(handle: usize, buf: &[u8], handles: &[u32]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_WRITE,
        handle,
        buf.as_ptr() as usize,
        buf.len(),
        handles.as_ptr() as usize,
        handles.len(),
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn exec_impl(name: &str) -> i32 {
    let mut len = name.len();
    let mut local_name = name.as_bytes();
    let ptr = local_name.as_ptr();
    syscall!(SYSCALL_EXEC, ptr as usize, len, 0, 0, 0, 0) as i32
}
