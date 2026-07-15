use super::{FileSystem, FsError};

extern crate libc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open(&self, path: &str) -> Result<i32, FsError> {
        let fd = libc::open(path.as_ptr(), 0, 0);
        if fd < 0 {
            Err(FsError::FileNotFound)
        } else {
            Ok(fd)
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, FsError> {
        let n = libc::read(fd, buf.as_mut_ptr(), buf.len());
        if n < 0 {
            Err(FsError::Unknown)
        } else {
            Ok(n as usize)
        }
    }

    fn close(&self, fd: i32) {
        let _ = libc::close(fd);
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = libc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = libc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        libc::exit(code);
    }
}
