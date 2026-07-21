use super::{Environment, ShellError};

extern crate libc;
extern crate libcapsule;
extern crate libstd;

pub struct CapsuleEnv;

impl Environment for CapsuleEnv {
    fn write_stdout(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn write_stderr(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        for i in 0..buf.len() {
            buf[i] = 0;
        }

        let res = libstd::io::read_line(buf);

        if res >= 0 {
            Ok(res as usize)
        } else {
            Err(ShellError::IoError)
        }
    }

    fn getcwd(&self, _buf: &mut [u8]) -> Result<usize, ShellError> {
        match libstd::env::current_dir() {
            Ok(s) => {
                // Return len of current dir
                Ok(s.len())
            }
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn chdir(&self, path: &str) -> Result<(), ShellError> {
        match libstd::env::set_current_dir(path) {
            Ok(()) => Ok(()),
            Err(_) => Err(ShellError::PathNotFound),
        }
    }

    fn get_env(&self, _key: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        Err(ShellError::PathNotFound)
    }

    fn set_env(&self, _key: &str, _value: &str) -> Result<(), ShellError> {
        Ok(())
    }

    fn print_envs(&self) {}

    fn execute(&self, cmd: &str, args: &[&str]) -> Result<i32, ShellError> {
        let mut cmd_buf = [0u8; 128];
        let cmd_bytes = cmd.as_bytes();
        let cmd_len = cmd_bytes.len().min(127);
        cmd_buf[..cmd_len].copy_from_slice(&cmd_bytes[..cmd_len]);
        cmd_buf[cmd_len] = 0;

        let mut args_storage = [[0u8; 128]; 16];
        let mut argv = [core::ptr::null::<u8>(); 17];

        args_storage[0][..cmd_len].copy_from_slice(&cmd_bytes[..cmd_len]);
        args_storage[0][cmd_len] = 0;
        argv[0] = args_storage[0].as_ptr();

        let count = core::cmp::min(args.len(), 15);
        for i in 0..count {
            let arg_bytes = args[i].as_bytes();
            let arg_len = arg_bytes.len().min(127);
            args_storage[i + 1][..arg_len].copy_from_slice(&arg_bytes[..arg_len]);
            args_storage[i + 1][arg_len] = 0;
            argv[i + 1] = args_storage[i + 1].as_ptr();
        }
        argv[count + 1] = core::ptr::null();

        let code = libc::execv(cmd_buf.as_ptr(), argv.as_ptr());
        Ok(code)
    }

    fn spawn(&self, cmd: &str, args: &[&str]) -> Result<u64, ShellError> {
        let mut cmd_buf = [0u8; 128];
        let cmd_bytes = cmd.as_bytes();
        let cmd_len = cmd_bytes.len().min(127);
        cmd_buf[..cmd_len].copy_from_slice(&cmd_bytes[..cmd_len]);
        cmd_buf[cmd_len] = 0;

        let fd = libc::open(cmd_buf.as_ptr(), 0, 0);
        if fd < 0 {
            return Err(ShellError::PathNotFound);
        }
        let mut st = core::mem::MaybeUninit::<libc::stat>::uninit();
        if libc::stat(cmd_buf.as_ptr(), st.as_mut_ptr()) < 0 {
            let _ = libc::close(fd);
            return Err(ShellError::PathNotFound);
        }
        let st = unsafe { st.assume_init() };
        let size = st.st_size as usize;
        if size == 0 {
            let _ = libc::close(fd);
            return Err(ShellError::IoError);
        }

        let binary_vmo = match libcapsule::syscalls::vmo_create(size) {
            Ok(h) => h,
            Err(_) => {
                let _ = libc::close(fd);
                return Err(ShellError::IoError);
            }
        };

        let mut buf = [0u8; 4096];
        let mut offset = 0;
        while offset < size {
            let want = core::cmp::min(buf.len(), size - offset);
            let n = libc::read(fd, buf.as_mut_ptr(), want);
            if n <= 0 {
                break;
            }
            if let Err(_) = libcapsule::syscalls::vmo_write(binary_vmo, offset, &buf[..n as usize]) {
                let _ = libcapsule::syscalls::close(binary_vmo);
                let _ = libc::close(fd);
                return Err(ShellError::IoError);
            }
            offset += n as usize;
        }
        let _ = libc::close(fd);

        let argv_vmo = match libcapsule::syscalls::vmo_create(4096) {
            Ok(h) => h,
            Err(_) => {
                let _ = libcapsule::syscalls::close(binary_vmo);
                return Err(ShellError::IoError);
            }
        };

        let mut argv_buf = [0u8; 4096];
        let count = core::cmp::min(args.len() + 1, 16);
        let count_bytes = (count as u32).to_le_bytes();
        argv_buf[0..4].copy_from_slice(&count_bytes);

        let mut off = 4;
        let arg0 = cmd.as_bytes();
        let arg0_len = arg0.len();
        let len_bytes0 = (arg0_len as u32).to_le_bytes();
        argv_buf[off..off+4].copy_from_slice(&len_bytes0);
        argv_buf[off+4..off+4+arg0_len].copy_from_slice(arg0);
        off += 4 + arg0_len;

        for i in 0..args.len() {
            if i + 1 >= 16 {
                break;
            }
            let arg = args[i].as_bytes();
            let arg_len = arg.len();
            if off + 4 + arg_len > argv_buf.len() {
                let _ = libcapsule::syscalls::close(binary_vmo);
                let _ = libcapsule::syscalls::close(argv_vmo);
                return Err(ShellError::IoError);
            }
            let len_bytes = (arg_len as u32).to_le_bytes();
            argv_buf[off..off+4].copy_from_slice(&len_bytes);
            argv_buf[off+4..off+4+arg_len].copy_from_slice(arg);
            off += 4 + arg_len;
        }

        if let Err(_) = libcapsule::syscalls::vmo_write(argv_vmo, 0, &argv_buf[..off]) {
            let _ = libcapsule::syscalls::close(binary_vmo);
            let _ = libcapsule::syscalls::close(argv_vmo);
            return Err(ShellError::IoError);
        }

        match libcapsule::syscalls::spawn(binary_vmo, argv_vmo) {
            Ok(pid) => {
                let _ = libcapsule::syscalls::close(binary_vmo);
                let _ = libcapsule::syscalls::close(argv_vmo);
                Ok(pid)
            }
            Err(_) => {
                let _ = libcapsule::syscalls::close(binary_vmo);
                let _ = libcapsule::syscalls::close(argv_vmo);
                Err(ShellError::IoError)
            }
        }
    }

    fn pipe(&self, fds: &mut [i32; 2]) -> Result<(), ShellError> {
        let mut raw = [0i32; 2];
        match libc::syscalls::pipe_pair(&mut raw) {
            Ok(()) => {
                fds[0] = raw[0];
                fds[1] = raw[1];
                Ok(())
            }
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn dup2(&self, old_fd: i32, new_fd: i32) -> Result<(), ShellError> {
        match libc::syscalls::dup2(old_fd, new_fd) {
            Ok(_) => Ok(()),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn wait(&self, pid: u64) -> Result<i32, ShellError> {
        let mut status: i32 = 0;
        loop {
            match libc::syscalls::wait4(pid as i64, &mut status, 0) {
                Ok((_, _)) => return Ok(status),
                Err(libcapsule::Status::TryAgain) => {
                    libstd::thread::yield_now();
                }
                Err(_) => return Err(ShellError::IoError),
            }
        }
    }

    fn yield_cpu(&self) {
        libstd::thread::yield_now();
    }

    fn open(&self, path: &str, _flags: i32) -> Result<i32, ShellError> {
        // Simple File Open using standard std File
        match libstd::fs::File::open(path) {
            Ok(file) => {
                let fd = file.fd;
                core::mem::forget(file);
                Ok(fd)
            }
            _ => Err(ShellError::PathNotFound),
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, ShellError> {
        let file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(fd) };
        let res = file.read(buf);
        core::mem::forget(file);
        match res {
            Ok(n) => Ok(n),
            _ => Err(ShellError::IoError),
        }
    }

    fn write(&self, fd: i32, data: &[u8]) -> Result<usize, ShellError> {
        let file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(fd) };
        let res = file.write(data);
        core::mem::forget(file);
        match res {
            Ok(n) => Ok(n),
            _ => Err(ShellError::IoError),
        }
    }

    fn read_file(&self, _path: &str, _buf: &mut [u8]) -> Result<usize, ShellError> {
        Err(ShellError::IoError)
    }

    fn exit(&self, _code: i32) -> ! {
        panic!("Shell exited");
    }
}
