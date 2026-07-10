use shared::status::Status;

#[macro_export]
macro_rules! syscall {
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let mut r0 = $a0 as usize;
        let r1 = $a1 as usize;
        let r2 = $a2 as usize;
        let r3 = $a3 as usize;
        let r4 = $a4 as usize;
        let r5 = $a5 as usize;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!(
                "svc #0",
                inout("x0") r0,
                in("x1") r1,
                in("x2") r2,
                in("x3") r3,
                in("x4") r4,
                in("x5") r5,
                in("x16") $num,
            );
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!(
                "ecall",
                inout("a0") r0,
                in("a1") r1,
                in("a2") r2,
                in("a3") r3,
                in("a4") r4,
                in("a5") r5,
                in("a7") $num,
            );
        }
        r0
    }};
}

pub const SYSCALL_EXIT: u32 = 0;
pub const SYSCALL_WRITE: u32 = 1;
pub const SYSCALL_GET_TID: u32 = 2;
pub const SYSCALL_GET_PID: u32 = 3;
pub const SYSCALL_CHANNEL_CREATE: u32 = 10;
pub const SYSCALL_CHANNEL_READ: u32 = 11;
pub const SYSCALL_CHANNEL_WRITE: u32 = 12;
pub const SYSCALL_CHANNEL_REGISTER: u32 = 14;
pub const SYSCALL_CHANNEL_LOOKUP: u32 = 15;
pub const SYSCALL_HANDLE_DUPLICATE: u32 = 16;
pub const SYSCALL_VMO_CREATE: u32 = 30;
pub const SYSCALL_VMO_READ: u32 = 31;
pub const SYSCALL_VMO_WRITE: u32 = 32;
pub const SYSCALL_OPEN: u32 = 100;
pub const SYSCALL_CLOSE: u32 = 101;
pub const SYSCALL_READ: u32 = 102;
pub const SYSCALL_GETCWD: u32 = 104;
pub const SYSCALL_CHDIR: u32 = 105;
pub const SYSCALL_EXEC: u32 = 110;
pub const SYSCALL_LOAD_BINARY: u32 = 111;

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
    let len = name.len();
    let local_name = name.as_bytes();
    let ptr = local_name.as_ptr();
    syscall!(SYSCALL_EXEC, ptr as usize, len, 0, 0, 0, 0) as i32
}

pub fn load_binary(vmo_handle: usize, name: &str) -> Result<u64, Status> {
    let ret = syscall!(
        SYSCALL_LOAD_BINARY,
        vmo_handle,
        name.as_ptr() as usize,
        name.len(),
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

pub fn channel_register(name: &str, handle: usize) -> Result<(), Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_REGISTER,
        name.as_ptr() as usize,
        name.len(),
        handle,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

pub fn channel_lookup(name: &str) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_CHANNEL_LOOKUP,
        name.as_ptr() as usize,
        name.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmo_create(size: usize) -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_VMO_CREATE, size, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmo_read(handle: usize, offset: usize, buf: &mut [u8]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_VMO_READ,
        handle,
        offset,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmo_write(handle: usize, offset: usize, buf: &[u8]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_VMO_WRITE,
        handle,
        offset,
        buf.as_ptr() as usize,
        buf.len(),
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn handle_duplicate(handle: usize, rights: u32) -> Result<usize, Status> {
    let ret = syscall!(SYSCALL_HANDLE_DUPLICATE, handle, rights, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn close(handle: usize) -> Result<(), Status> {
    let ret = syscall!(SYSCALL_CLOSE, handle, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

pub fn getcwd(buf: &mut [u8]) -> Result<usize, Status> {
    let ret = syscall!(
        SYSCALL_GETCWD,
        buf.as_mut_ptr() as usize,
        buf.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn chdir(path: &str) -> Result<(), Status> {
    let ret = syscall!(
        SYSCALL_CHDIR,
        path.as_ptr() as usize,
        path.len(),
        0,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}
