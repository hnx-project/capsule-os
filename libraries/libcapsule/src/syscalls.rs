use shared::status::{Result, Status};
#[allow(unused_imports)]
pub use shared::syscall_nums::*;

#[macro_export]
macro_rules! syscall {
    ($num:expr, $a0:expr, $a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr) => {{
        let r0_tmp = $a0 as usize;
        let r1 = $a1 as usize;
        let r2 = $a2 as usize;
        let r3 = $a3 as usize;
        let r4 = $a4 as usize;
        let r5 = $a5 as usize;
        let mut ret_val: usize;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!(
                "mov x0, {tmp}",
                "svc #0",
                "mov {tmp2}, x0",
                tmp = in(reg) r0_tmp,
                tmp2 = out(reg) ret_val,
                in("x1") r1,
                in("x2") r2,
                in("x3") r3,
                in("x4") r4,
                in("x5") r5,
                in("x16") $num,
                out("x30") _,             // Tell the compiler that the Link Register (x30) is clobbered!
                options(nostack)          // Enforce that memory is completely clean
            );
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!(
                "mv a0, {tmp}",
                "ecall",
                "mv {tmp2}, a0",
                tmp = in(reg) r0_tmp,
                tmp2 = out(reg) ret_val,
                in("a1") r1,
                in("a2") r2,
                in("a3") r3,
                in("a4") r4,
                in("a5") r5,
                in("a7") $num,
                out("ra") _,
                options(nostack)
            );
        }
        ret_val
    }};
}

pub fn channel_create() -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_CREATE,
        0,
        0,
        0,
        0,
        0,
        0
    );
    if ret == 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn channel_read(handle: usize, buf: &mut [u8], handles: &mut [u32]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_READ,
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

pub fn channel_write(handle: usize, buf: &[u8], handles: &[u32]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_WRITE,
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

pub fn channel_register(name: &str, handle: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_REGISTER,
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

pub fn channel_lookup(name: &str) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_CHANNEL_LOOKUP,
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

pub fn close(handle: usize) -> Result<()> {
    let ret = syscall!(shared::syscall_nums::SYSCALL_CLOSE, handle, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(())
    }
}

pub fn yield_cpu() -> isize {
    syscall!(shared::syscall_nums::SYSCALL_YIELD, 0, 0, 0, 0, 0, 0) as isize
}

pub fn vmo_create(size: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_CREATE,
        size,
        0,
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

pub fn vmo_create_child(parent_handle: usize, offset: usize, size: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_CREATE_CHILD,
        parent_handle,
        offset,
        size,
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

pub fn vmo_read(handle: usize, offset: usize, buf: &mut [u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_READ,
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

pub fn vmo_write(handle: usize, offset: usize, buf: &[u8]) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMO_WRITE,
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

pub fn handle_duplicate(handle: usize, rights: u32) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_HANDLE_DUPLICATE,
        handle,
        rights as usize,
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

pub fn vmar_map_self(
    vmo_handle: usize,
    vaddr_offset: usize,
    size: usize,
    flags: u32,
) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMAR_MAP_SELF,
        vmo_handle,
        vaddr_offset,
        size,
        flags as usize,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn vmar_unmap(vaddr_offset: usize, size: usize) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_VMAR_UNMAP,
        vaddr_offset,
        size,
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

pub fn process_create(name: &str) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_PROCESS_CREATE,
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

pub fn load_binary(vmo_handle: usize, name: &str) -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_LOAD_BINARY,
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

pub fn wait4(pid: i64, status_ptr: *mut i32, options: i32) -> Result<u64> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_WAIT4,
        pid as usize,
        status_ptr as usize,
        options as usize,
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

pub fn service_spawn(desc: &shared::launcher::ServiceDescriptor) -> Result<u64> {
    // Stage 1: print info before entering syscall
    let desc_ptr = desc as *const shared::launcher::ServiceDescriptor as usize;

    // We cannot use normal formatting here easily without allocator/format macros,
    // so we write a raw print using a simple block.
    // Let's write the syscall with a simple wrapper to see if we ever get back.
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_SERVICE_SPAWN,
        desc_ptr,
        0,
        0,
        0,
        0,
        0
    );

    // If we reach here, we're extremely lucky! Let's print the return value as raw bytes
    // or through some syscall.
    let mut temp = [0u8; 32];
    temp[0..13].copy_from_slice(b"SPAWN RETURN ");
    let mut val = ret;
    for i in 0..8 {
        let b = (val & 0xF) as u8;
        temp[13 + 7 - i] = if b < 10 { b'0' + b } else { b'A' + (b - 10) };
        val >>= 4;
    }
    temp[21] = b'\n';
    let _ = syscall!(
        shared::syscall_nums::SYSCALL_WRITE,
        1,
        temp.as_ptr() as usize,
        22,
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

pub fn spawn(path: &str, argv: &[&[u8]]) -> Result<u64> {
    let path_ptr = path.as_bytes().as_ptr();
    let path_len = path.len();
    let argc = argv.len();

    let mut pairs: [[u8; 16]; 16] = [[0u8; 16]; 16];
    for (i, arg) in argv.iter().enumerate() {
        if i >= 16 {
            break;
        }
        let ptr_bytes = (arg.as_ptr() as u64).to_le_bytes();
        let len_bytes = (arg.len() as u64).to_le_bytes();
        pairs[i][0..8].copy_from_slice(&ptr_bytes);
        pairs[i][8..16].copy_from_slice(&len_bytes);
    }
    let argv_ptr = if argc > 0 { pairs.as_ptr() as usize } else { 0 };

    let ret = syscall!(
        shared::syscall_nums::SYSCALL_SPAWN,
        path_ptr as usize,
        path_len,
        argv_ptr,
        argc,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret as u64)
    }
}

pub fn proc_mgmt(cmd: u32, arg1: usize, arg2: usize, arg3: usize) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_PROC_MGMT,
        cmd as usize,
        arg1,
        arg2,
        arg3,
        0,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn thread_create(
    process_handle: u32,
    entry_pc: usize,
    arg0: usize,
    arg1: usize,
    stack_size: usize,
) -> Result<usize> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_THREAD_CREATE,
        process_handle as usize,
        entry_pc,
        arg0,
        arg1,
        stack_size,
        0
    );
    if (ret as isize) < 0 {
        Err(Status::from_raw(ret as i32))
    } else {
        Ok(ret)
    }
}

pub fn thread_start(thread_handle: u32) -> Result<()> {
    let ret = syscall!(
        shared::syscall_nums::SYSCALL_THREAD_START,
        thread_handle as usize,
        0,
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
