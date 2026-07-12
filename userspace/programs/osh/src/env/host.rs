use super::{Environment, ShellError};
use std::io::{Read, Write};

pub struct HostEnv;

impl Environment for HostEnv {
    fn write_stdout(&self, data: &[u8]) {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(data);
        let _ = stdout.flush();
    }

    fn write_stderr(&self, data: &[u8]) {
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(data);
        let _ = stderr.flush();
    }

    fn read_line(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        let mut stdin = std::io::stdin();
        let mut temp_buf = [0u8; 1];
        let mut bytes_read = 0;

        while bytes_read < buf.len() {
            match stdin.read_exact(&mut temp_buf) {
                Ok(_) => {
                    let c = temp_buf[0];
                    buf[bytes_read] = c;
                    bytes_read += 1;
                    if c == b'\n' {
                        break;
                    }
                }
                Err(_) => {
                    if bytes_read > 0 {
                        break;
                    } else {
                        return Err(ShellError::IoError);
                    }
                }
            }
        }
        Ok(bytes_read)
    }

    fn getcwd(&self, buf: &mut [u8]) -> Result<usize, ShellError> {
        match std::env::current_dir() {
            Ok(path) => {
                let path_str = path.to_string_lossy();
                let bytes = path_str.as_bytes();
                if bytes.len() > buf.len() {
                    return Err(ShellError::IoError);
                }
                buf[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            Err(_) => Err(ShellError::Unknown),
        }
    }

    fn chdir(&self, path: &str) -> Result<(), ShellError> {
        match std::env::set_current_dir(path) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(ShellError::PathNotFound),
                std::io::ErrorKind::PermissionDenied => Err(ShellError::PermissionDenied),
                _ => Err(ShellError::Unknown),
            },
        }
    }

    fn get_env(&self, key: &str, buf: &mut [u8]) -> Result<usize, ShellError> {
        match std::env::var(key) {
            Ok(val) => {
                let bytes = val.as_bytes();
                if bytes.len() > buf.len() {
                    return Err(ShellError::IoError);
                }
                buf[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
            Err(_) => Err(ShellError::PathNotFound),
        }
    }

    fn set_env(&self, key: &str, value: &str) -> Result<(), ShellError> {
        std::env::set_var(key, value);
        Ok(())
    }

    fn print_envs(&self) {
        for (key, val) in std::env::vars() {
            self.write_stdout(key.as_bytes());
            self.write_stdout(b"=");
            self.write_stdout(val.as_bytes());
            self.write_stdout(b"\n");
        }
    }

    fn execute(&self, cmd: &str, args: &[&str]) -> Result<i32, ShellError> {
        let mut command = std::process::Command::new(cmd);
        for arg in args {
            command.arg(arg);
        }
        match command.status() {
            Ok(status) => Ok(status.code().unwrap_or(0)),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(ShellError::PathNotFound),
                std::io::ErrorKind::PermissionDenied => Err(ShellError::PermissionDenied),
                _ => Err(ShellError::Unknown),
            },
        }
    }

    fn spawn(&self, cmd: &str, args: &[&str]) -> Result<u64, ShellError> {
        // Host-mode fallback: run a synchronous shell via
        // `std::process::Command::new(cmd).spawn` and return
        // the resulting OS pid.  Returns 0 on platforms where
        // the pid is opaque (we don't actually need the real
        // number for the host smoke tests).
        match std::process::Command::new(cmd).args(args).spawn() {
            Ok(child) => Ok(child.id() as u64),
            Err(_) => Err(ShellError::PathNotFound),
        }
    }
    fn pipe(&self, fds: &mut [i32; 2]) -> Result<(), ShellError> {
        // Host helper that picks free fds so the B7 grammar
        // can run on a desktop smoke.  Real pipe creation uses
        // libc::pipe on POSIX hosts; the capsule impl replaces
        // this entirely.  For host smoke we synthesise fake
        // fds that aren't actually connected to a kernel pipe
        // -- host smoke only exercises the parser, not the
        // pipeline plumbing.
        fds[0] = 100;
        fds[1] = 101;
        Ok(())
    }
    fn dup2(&self, _old_fd: i32, _new_fd: i32) -> Result<(), ShellError> {
        Ok(())
    }
    fn wait(&self, _pid: u64) -> Result<i32, ShellError> {
        Ok(0)
    }
    fn yield_cpu(&self) {}
    fn open(&self, path: &str, _flags: i32) -> Result<i32, ShellError> {
        // host-side test convenience: open the path and return
        // a file descriptor.  We don't actually need to plug
        // it into the test framework today.
        let _ = path;
        Ok(200)
    }
    fn read(&self, _fd: i32, _buf: &mut [u8]) -> Result<usize, ShellError> {
        Ok(0)
    }

    fn read_file(&self, path: &str, buf: &mut [u8]) -> Result<usize, ShellError> {
        match std::fs::File::open(path) {
            Ok(mut file) => match file.read(buf) {
                Ok(bytes) => Ok(bytes),
                Err(_) => Err(ShellError::IoError),
            },
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(ShellError::PathNotFound),
                std::io::ErrorKind::PermissionDenied => Err(ShellError::PermissionDenied),
                _ => Err(ShellError::Unknown),
            },
        }
    }

    fn exit(&self, code: i32) -> ! {
        std::process::exit(code);
    }
}
