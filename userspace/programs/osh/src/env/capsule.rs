use super::{Environment, ShellError};

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
        // Since osh is the interactive platform shell, we allow it to spawn processes.
        // It bypasses core apps restriction by explicitly linking `libcapsule`.
        let mut argv_storage: [&[u8]; 16] = [&[]; 16];
        argv_storage[0] = cmd.as_bytes();
        for (i, a) in args.iter().enumerate() {
            if i + 1 >= 16 {
                break;
            }
            argv_storage[i + 1] = a.as_bytes();
        }
        let count = core::cmp::min(args.len() + 1, 16);
        // Fallback or use libcapsule's clean execve wrapper
        let code = libcapsule::syscalls::execve_impl(cmd, &argv_storage[..count]);
        Ok(code)
    }

    fn spawn(&self, cmd: &str, args: &[&str]) -> Result<u64, ShellError> {
        let mut argv_storage: [&[u8]; 16] = [&[]; 16];
        argv_storage[0] = cmd.as_bytes();
        for (i, a) in args.iter().enumerate() {
            if i + 1 >= 16 {
                break;
            }
            argv_storage[i + 1] = a.as_bytes();
        }
        let count = core::cmp::min(args.len() + 1, 16);
        match libcapsule::syscalls::spawn(cmd, &argv_storage[..count]) {
            Ok(pid) => Ok(pid),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn pipe(&self, fds: &mut [i32; 2]) -> Result<(), ShellError> {
        let mut raw = [0i32; 2];
        match libcapsule::syscalls::pipe_pair(&mut raw) {
            Ok(()) => {
                fds[0] = raw[0];
                fds[1] = raw[1];
                Ok(())
            }
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn dup2(&self, old_fd: i32, new_fd: i32) -> Result<(), ShellError> {
        match libcapsule::syscalls::dup2(old_fd, new_fd) {
            Ok(_) => Ok(()),
            Err(_) => Err(ShellError::IoError),
        }
    }

    fn wait(&self, pid: u64) -> Result<i32, ShellError> {
        let mut status: i32 = 0;
        loop {
            match libcapsule::syscalls::wait4(pid as i64, &mut status, 0) {
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
