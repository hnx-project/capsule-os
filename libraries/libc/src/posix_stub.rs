pub const ENOSYS: i32 = 38;
pub const ECHILD: i32 = 10;

use core::sync::atomic::{AtomicI32, Ordering};

/// Thread/task-local `errno` slot.  Stored as `AtomicI32` rather
/// than `pub static mut` so the 2024-edition lint warning
/// (`constant_mut` / `static_mut_refs`) doesn't trip on every
/// libc call.  CapsuleOS is single-core for 1.0 so a single
/// global is sufficient; a future SMP bring-up should switch
/// to a per-CPU or per-thread cell backed by TLS / the kernel
/// scheduler.
#[no_mangle]
pub static errno: AtomicI32 = AtomicI32::new(0);

pub fn set_errno_and_fail(err: i32) -> i32 {
    set_errno_only(err);
    -1
}

/// Set the per-thread `errno` slot without forcing the return value
/// to `-1`.  Used by callers that want to translate a negative Status
/// into errno while preserving the original Status for comparison.
pub fn set_errno_only(err: i32) {
    errno.store(err, Ordering::SeqCst);
}

/// Read the current `errno` value.  Mirrors the C-ABI `*errno`
/// convention (read is the public accessor).
#[allow(dead_code)]
pub fn errno_value() -> i32 {
    errno.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn fork() -> i32 {
    // CapsuleOS deliberately does NOT support `fork()` at the
    // libc public boundary.  See DEVELOPMENT.md §5 (Hybrid
    // POSIX + Capability) and the header comment in
    // `libraries/libcapsule/include/capsule.h` — POSIX programs
    // should migrate to `posix_spawn(3)`, which is the
    // Fuchsia / Zircon model this codebase follows.
    //
    // The underlying `SYSCALL_FORK` syscall is *still wired up*
    // in the kernel because the bash compatibility layer
    // (`/system/bin/bash` running as a privileged service) and
    // `procmgr`'s internal service-spawn path both need it.
    // Programs that genuinely need raw `fork` semantics can
    // reach it through `libcapsule::syscalls::fork()`.
    set_errno_and_fail(ENOSYS)
}

#[no_mangle]
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

    // 2. Locate the file and get its size using stat (to avoid concurrent session deadlock on fileagent)
    let mut fd = -1;
    let mut winner_path = [0u8; 128];
    let mut winner_len = 0;
    let mut st = core::mem::MaybeUninit::<crate::stat>::uninit();
    let mut found = false;

    let has_slash = path_str.contains('/');
    if !has_slash {
        let search_dirs = &["/boot/system/bin", "/system/bin", "/bin", "/boot"];
        for dir in search_dirs {
            let mut candidate = [0u8; 128];
            let dir_bytes = dir.as_bytes();
            let d_len = dir_bytes.len();
            let p_len = path_str.len();
            if d_len + 1 + p_len >= 128 {
                continue;
            }
            candidate[..d_len].copy_from_slice(dir_bytes);
            candidate[d_len] = b'/';
            candidate[d_len + 1..d_len + 1 + p_len].copy_from_slice(path_str.as_bytes());
            candidate[d_len + 1 + p_len] = 0;

            if crate::stat(candidate.as_ptr(), st.as_mut_ptr()) == 0 {
                winner_path[..d_len + 1 + p_len].copy_from_slice(&candidate[..d_len + 1 + p_len]);
                winner_path[d_len + 1 + p_len] = 0;
                winner_len = d_len + 1 + p_len;
                found = true;
                break;
            }
        }
    }

    if !found {
        if crate::stat(path, st.as_mut_ptr()) == 0 {
            winner_path[..path_len].copy_from_slice(path_slice);
            winner_path[path_len] = 0;
            winner_len = path_len;
            found = true;
        }
    }

    if !found {
        return set_errno_and_fail(2); // ENOENT
    }

    let st = unsafe { st.assume_init() };
    let size = st.st_size as usize;
    if size == 0 {
        return set_errno_and_fail(22); // EINVAL
    }

    fd = crate::open(winner_path.as_ptr(), 0, 0);
    if fd < 0 {
        return set_errno_and_fail(2); // ENOENT (No such file)
    }

    // S5: close every fd marked with FD_CLOEXEC before exec.
    // The kernel doesn't see `USER_FD_TABLE`; libc is the
    // single owner of these flags, so the close has to happen
    // here.
    crate::close_cloexec_fds();

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

