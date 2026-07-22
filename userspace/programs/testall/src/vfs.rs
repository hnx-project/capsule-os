use libc;
use libcapsule::syscalls;

pub fn test_connect() -> bool {
    match syscalls::channel_lookup("svc.vfs") {
        Ok(ch) => {
            let _ = syscalls::close(ch);
            true
        }
        Err(_) => false,
    }
}

pub fn posix_open(path: &str) -> i32 {
    let flags = if path == "/no_such_file" || path == "/dev/tty" || path == "/tmp" || path.starts_with("/dev/") { 0 } else { 0x40 };
    libc::open_str(path, flags, 0)
}

pub fn posix_close(fd: i32) -> i32 {
    libc::close(fd)
}

pub fn posix_read(fd: i32, buf: &mut [u8]) -> isize {
    libc::read(fd, buf.as_mut_ptr(), buf.len())
}

pub fn posix_write(fd: i32, data: &[u8]) -> isize {
    libc::write(fd, data.as_ptr(), data.len())
}

pub fn posix_mkdir(path: &str) -> i32 {
    let mut buf = [0u8; 256];
    let len = path.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&path.as_bytes()[..len]);
    libc::mkdir(buf.as_ptr())
}

pub fn posix_rmdir(path: &str) -> i32 {
    let mut buf = [0u8; 256];
    let len = path.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&path.as_bytes()[..len]);
    libc::rmdir(buf.as_ptr())
}

pub fn posix_unlink(path: &str) -> i32 {
    let mut buf = [0u8; 256];
    let len = path.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&path.as_bytes()[..len]);
    libc::unlink(buf.as_ptr())
}

pub fn posix_stat(path: &str) -> (i64, u32) {
    let mut buf = [0u8; 256];
    let len = path.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&path.as_bytes()[..len]);
    
    let mut s = libc::stat { st_size: 0, st_mode: 0 };
    let ret = libc::stat(buf.as_ptr(), &mut s as *mut libc::stat);
    if ret == 0 {
        let ntype = if s.st_mode == 0x4000 { 2 } else if s.st_mode == 0x8000 { 1 } else { 0 };
        (s.st_size, ntype)
    } else {
        (-1, 0)
    }
}

pub fn posix_readdir(path: &str, buf: &mut [u8]) -> isize {
    let fd = posix_open(path);
    if fd < 0 {
        return -1;
    }
    let n = unsafe { libc::readdir(fd, buf.as_mut_ptr(), buf.len()) };
    posix_close(fd);
    n
}

pub fn test_create_file() -> bool {
    let fd = posix_open("/testfile.txt");
    if fd < 0 {
        return false;
    }
    let n = posix_write(fd, b"Hello VFS!");
    n == 10 && posix_close(fd) == 0
}

pub fn test_read_file() -> bool {
    let fd = posix_open("/testfile.txt");
    if fd < 0 {
        return false;
    }
    let mut buf = [0u8; 128];
    let size = posix_read(fd, &mut buf);
    if size != 10 {
        return false;
    }
    let expected = b"Hello VFS!";
    let ok = &buf[..size as usize] == expected;
    posix_close(fd) == 0 && ok
}

pub fn test_mkdir() -> bool {
    posix_mkdir("/tmp") == 0 && posix_mkdir("/tmp/a") == 0 && posix_mkdir("/tmp/a/b") == 0
}

pub fn test_mkdir_dup() -> bool {
    posix_mkdir("/tmp/a") == libcapsule::Status::AlreadyExists.to_raw() as i32
}

pub fn test_readdir() -> bool {
    let mut buf = [0u8; 128];
    let size = posix_readdir("/tmp", &mut buf);
    if size != 128 {
        return false;
    }
    let dirent = unsafe { &*(buf.as_ptr() as *const libc::Dirent) };
    if dirent.ftype != 2 {
        return false;
    }
    let name_len = dirent.name_len as usize;
    if name_len == 0 || name_len > 110 {
        return false;
    }
    let name_str = core::str::from_utf8(&dirent.name[..name_len]).unwrap_or("");
    name_str == "a"
}

pub fn test_rmdir() -> bool {
    posix_rmdir("/tmp/a/b") == 0
}

pub fn test_unlink() -> bool {
    posix_unlink("/testfile.txt") == 0
}

pub fn test_open_nonexist() -> bool {
    posix_open("/no_such_file") < 0
}

pub fn test_stat_root() -> bool {
    let (size, ntype) = posix_stat("/");
    size >= 0 && ntype == 2
}

pub fn test_stat_file() -> bool {
    let fd = posix_open("/testfile2.txt");
    if fd < 0 {
        return false;
    }
    let n = posix_write(fd, b"test data");
    if n != 9 {
        return false;
    }
    posix_close(fd);
    let (size, ntype) = posix_stat("/testfile2.txt");
    let ok = size == 9 && ntype == 1;
    posix_unlink("/testfile2.txt");
    ok
}

pub fn test_tty() -> bool {
    let fd = posix_open("/dev/tty");
    let ok = fd >= 0;
    if ok {
        posix_close(fd);
    }
    ok
}

pub fn test_dev_open_pl011() -> bool {
    let fd = posix_open("/dev/pl011");
    if fd < 0 {
        return false;
    }
    posix_close(fd) == 0
}

pub fn test_dev_open_nonexist() -> bool {
    posix_open("/dev/no_such_device") < 0
}

pub fn test_dev_read_pl011() -> bool {
    let fd = posix_open("/dev/pl011");
    if fd < 0 {
        return false;
    }
    let mut buf = [0u8; 4];
    let n = posix_read(fd, &mut buf);
    let ok = n == 4;
    posix_close(fd);
    ok
}

pub fn test_dev_write_pl011() -> bool {
    let fd = posix_open("/dev/pl011");
    if fd < 0 {
        return false;
    }
    let data = [0x00u8, 0x00, 0x00, 0x00];
    let n = posix_write(fd, &data);
    let ok = n == 4;
    posix_close(fd);
    ok
}

pub fn test_boot_create_file() -> bool {
    let fd = posix_open("/boot/test.txt");
    if fd < 0 {
        return false;
    }
    let n = posix_write(fd, b"FatFS Rules!");
    n == 12 && posix_close(fd) == 0
}

pub fn test_boot_read_file() -> bool {
    let fd = posix_open("/boot/test.txt");
    if fd < 0 {
        return false;
    }
    let mut buf = [0u8; 128];
    let size = posix_read(fd, &mut buf);
    if size != 12 {
        return false;
    }
    let expected = b"FatFS Rules!";
    let ok = &buf[..size as usize] == expected;
    posix_close(fd) == 0 && ok
}

pub fn test_boot_mkdir() -> bool {
    posix_mkdir("/boot/testdir") == 0
}

pub fn test_boot_unlink() -> bool {
    posix_unlink("/boot/test.txt") == 0 && posix_rmdir("/boot/testdir") == 0
}

pub fn test_cwd_getcwd() -> bool {
    let mut buf = [0u8; 256];
    match libc::getcwd(&mut buf) {
        Ok(len) => {
            let s = core::str::from_utf8(&buf[..len]).unwrap_or("");
            s == "/"
        }
        Err(_) => false,
    }
}

pub fn test_cwd_relative() -> bool {
    if libc::chdir("/tmp").is_err() {
        return false;
    }

    let mut buf = [0u8; 256];
    match libc::getcwd(&mut buf) {
        Ok(len) => {
            let s = core::str::from_utf8(&buf[..len]).unwrap_or("");
            if s != "/tmp" {
                let _ = libc::chdir("/");
                return false;
            }
        }
        Err(_) => {
            let _ = libc::chdir("/");
            return false;
        }
    }

    if posix_mkdir("test_cwd_dir") != 0 {
        let _ = libc::chdir("/");
        return false;
    }

    let (size, ntype) = posix_stat("/tmp/test_cwd_dir");
    if size < 0 || ntype != 2 {
        let _ = posix_rmdir("/tmp/test_cwd_dir");
        let _ = libc::chdir("/");
        return false;
    }

    if posix_rmdir("test_cwd_dir") != 0 {
        let _ = libc::chdir("/");
        return false;
    }

    libc::chdir("/").is_ok()
}
