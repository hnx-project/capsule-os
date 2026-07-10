use super::{FileSystem, FsError};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};

pub struct HostEnv;

impl FileSystem for HostEnv {
    fn open(&self, path: &str) -> Result<i32, FsError> {
        match File::open(path) {
            Ok(file) => Ok(file.into_raw_fd() as i32),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(FsError::FileNotFound),
                std::io::ErrorKind::PermissionDenied => Err(FsError::PermissionDenied),
                _ => Err(FsError::Unknown),
            },
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, FsError> {
        let mut file = unsafe { File::from_raw_fd(fd as RawFd) };
        let res = file.read(buf);
        // 保持 RawFd 所有权不被析构关闭，转移回去
        let _ = file.into_raw_fd();

        match res {
            Ok(bytes_read) => Ok(bytes_read),
            Err(_) => Err(FsError::Unknown),
        }
    }

    fn close(&self, fd: i32) {
        let _ = unsafe { File::from_raw_fd(fd as RawFd) };
        // 自动析构安全关闭 fd
    }

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

    fn exit(&self, code: i32) -> ! {
        std::process::exit(code);
    }
}
