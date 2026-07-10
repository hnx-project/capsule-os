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
