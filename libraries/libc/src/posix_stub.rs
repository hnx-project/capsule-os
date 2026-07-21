pub const ENOSYS: i32 = 38;
pub const ECHILD: i32 = 10;

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
pub extern "C" fn execv(path: *const u8, argv: *const *const u8) -> i32 {
    if path.is_null() {
        return set_errno_and_fail(2); // ENOENT (No such file)
    }

    // 1. Safe scan of path C-string
    let mut path_len = 0;
    unsafe {
        while *path.add(path_len) != 0 && path_len < 128 {
            path_len += 1;
        }
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path, path_len) };
    let path_str = match core::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return set_errno_and_fail(22), // EINVAL
    };

    // 2. Open the file to verify it exists and read it
    let fd = crate::open(path, 0, 0);
    if fd < 0 {
        return set_errno_and_fail(2); // ENOENT (No such file)
    }

    let mut st = core::mem::MaybeUninit::<crate::stat>::uninit();
    if crate::stat(path, st.as_mut_ptr()) < 0 {
        let _ = crate::close(fd);
        return set_errno_and_fail(2); // ENOENT
    }
    let st = unsafe { st.assume_init() };
    let size = st.st_size as usize;
    if size == 0 {
        let _ = crate::close(fd);
        return set_errno_and_fail(22); // EINVAL
    }

    // Create a binary VMO capability
    let vmo_handle = match libcapsule::syscalls::vmo_create(size) {
        Ok(h) => h,
        Err(_) => {
            let _ = crate::close(fd);
            return set_errno_and_fail(12); // ENOMEM
        }
    };

    // Read file bytes into the binary VMO
    let mut buf = [0u8; 4096];
    let mut offset = 0;
    while offset < size {
        let want = core::cmp::min(buf.len(), size - offset);
        let n = crate::read(fd, buf.as_mut_ptr(), want);
        if n <= 0 {
            break;
        }
        if let Err(_) = libcapsule::syscalls::vmo_write(vmo_handle, offset, &buf[..n as usize]) {
            let _ = libcapsule::syscalls::close(vmo_handle);
            let _ = crate::close(fd);
            return set_errno_and_fail(5); // EIO
        }
        offset += n as usize;
    }
    let _ = crate::close(fd);

    // 3. Scan argv pointer array
    let mut args_storage = [&[0u8; 0] as &[u8]; 16];
    let mut count = 0;
    if !argv.is_null() {
        unsafe {
            while !(*argv.add(count)).is_null() && count < 16 {
                let arg_ptr = *argv.add(count);
                let mut arg_len = 0;
                while *arg_ptr.add(arg_len) != 0 && arg_len < 256 {
                    arg_len += 1;
                }
                args_storage[count] = core::slice::from_raw_parts(arg_ptr, arg_len);
                count += 1;
            }
        }
    }

    // Create argv VMO capability
    let argv_vmo_handle = match libcapsule::syscalls::vmo_create(4096) {
        Ok(h) => h,
        Err(_) => {
            let _ = libcapsule::syscalls::close(vmo_handle);
            return set_errno_and_fail(12); // ENOMEM
        }
    };

    // Serialize argv into VMO
    let mut argv_buf = [0u8; 4096];
    let count_bytes = (count as u32).to_le_bytes();
    argv_buf[0..4].copy_from_slice(&count_bytes);
    let mut offset = 4;
    for i in 0..count {
        let arg = args_storage[i];
        let arg_len = arg.len();
        if offset + 4 + arg_len > argv_buf.len() {
            let _ = libcapsule::syscalls::close(vmo_handle);
            let _ = libcapsule::syscalls::close(argv_vmo_handle);
            return set_errno_and_fail(7); // E2BIG
        }
        let len_bytes = (arg_len as u32).to_le_bytes();
        argv_buf[offset..offset+4].copy_from_slice(&len_bytes);
        argv_buf[offset+4..offset+4+arg_len].copy_from_slice(arg);
        offset += 4 + arg_len;
    }

    if let Err(_) = libcapsule::syscalls::vmo_write(argv_vmo_handle, 0, &argv_buf[..offset]) {
        let _ = libcapsule::syscalls::close(vmo_handle);
        let _ = libcapsule::syscalls::close(argv_vmo_handle);
        return set_errno_and_fail(5); // EIO
    }

    // 4. Invoke pure capability process replacement
    match libcapsule::syscalls::execve(vmo_handle, argv_vmo_handle) {
        Ok(()) => 0,
        Err(e) => {
            let _ = libcapsule::syscalls::close(vmo_handle);
            let _ = libcapsule::syscalls::close(argv_vmo_handle);
            set_errno_and_fail(e.to_raw() as i32)
        }
    }
}

#[no_mangle]
pub extern "C" fn wait4(pid: i32, status: *mut i32, _options: i32, _rusage: *mut u8) -> i32 {
    let ret = libcapsule::syscall!(
        shared::syscall_nums::SYSCALL_WAIT4,
        pid as usize,
        status as usize,
        _options as usize,
        0,
        0,
        0
    );
    if (ret as isize) < 0 {
        set_errno_and_fail(ECHILD)
    } else {
        ret as i32
    }
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
