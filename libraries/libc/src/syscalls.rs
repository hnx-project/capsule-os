use shared::status::Status;
#[allow(unused_imports)]
pub use shared::syscall_nums::*;
extern crate libcapsule;

pub fn exit(code: i32) -> ! {
    libcapsule::syscall!(SYSCALL_EXIT, code as usize, 0, 0, 0, 0, 0);
    loop {}
}

pub fn write_fd(fd: usize, ptr: usize, len: usize) -> usize {
    libcapsule::syscall!(SYSCALL_WRITE, fd, ptr, len, 0, 0, 0)
}

pub fn exec_impl(name: &str) -> i32 {
    let len = name.len();
    let local_name = name.as_bytes();
    let ptr = local_name.as_ptr();
    libcapsule::syscall!(SYSCALL_EXEC, ptr as usize, len, 0, 0, 0, 0) as i32
}

pub fn execve_impl(path: &str, argv: &[&[u8]]) -> i32 {
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

    libcapsule::syscall!(
        SYSCALL_EXECVE,
        path_ptr as usize,
        path_len,
        argv_ptr,
        argc,
        0,
        0
    ) as i32
}

pub fn getcwd(buf: &mut [u8]) -> Result<usize, Status> {
    let ret = libcapsule::syscall!(
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
    let ret = libcapsule::syscall!(
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

pub fn wait4(pid: i64, status_ptr: *mut i32, options: i32) -> Result<(u64, i32), Status> {
    let ret = libcapsule::syscall!(
        SYSCALL_WAIT4,
        pid as usize,
        status_ptr as usize,
        options as usize,
        0,
        0,
        0
    );
    if (ret as i64) < 0 {
        let status = Status::from_raw(ret as i32);
        return Err(status);
    }
    Ok((ret as u64, 0))
}

pub fn getppid() -> Result<u64, Status> {
    let ret = libcapsule::syscall!(SYSCALL_GET_PID, 0, 0, 0, 0, 0, 0);
    if (ret as i64) < 0 {
        let s = Status::from_raw(ret as i32);
        Err(s)
    } else {
        Ok(ret as u64)
    }
}

pub fn sigaction(
    sig: usize,
    sa_handler: usize,
    mask: usize,
    flags: usize,
) -> Result<usize, Status> {
    let ret = libcapsule::syscall!(SYSCALL_SIGACTION, sig, sa_handler, mask, flags, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(ret)
}

pub fn raise(sig: usize) -> Result<(), Status> {
    let ret = libcapsule::syscall!(SYSCALL_RAISE, sig, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

pub fn kill(pid: i64, sig: usize) -> Result<(), Status> {
    let ret = libcapsule::syscall!(SYSCALL_KILL, pid as usize, sig, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

pub fn pause() -> Result<(), Status> {
    let ret = libcapsule::syscall!(SYSCALL_PAUSE, 0, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

pub fn pipe(ufds_ptr: *mut i32) -> Result<(), Status> {
    let ret = libcapsule::syscall!(SYSCALL_PIPE, ufds_ptr as usize, 0, 0, 0, 0, 0);
    if (ret as isize) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(())
}

pub fn pipe_pair(ufds: &mut [i32; 2]) -> Result<(), Status> {
    pipe(ufds.as_mut_ptr())
}

pub fn dup2(oldfd: i32, newfd: i32) -> Result<i32, Status> {
    let ret = libcapsule::syscall!(SYSCALL_DUP2, oldfd as usize, newfd as usize, 0, 0, 0, 0);
    if (ret as i64) < 0 {
        let s = Status::from_raw(ret as i32);
        return Err(s);
    }
    Ok(ret as i32)
}
