use super::{Dirent, FileSystem, FsError};

extern crate libc;
extern crate libstd;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open_dir(&self, path: &str) -> Result<i32, FsError> {
        let fd = libc::open_str(path, 0, 0);
        if fd >= 0 {
            Ok(fd)
        } else {
            Err(FsError::DirectoryNotFound)
        }
    }

    fn readdir(&self, fd: i32, dirent: &mut Dirent) -> Result<bool, FsError> {
        let ptr = dirent as *mut Dirent as *mut u8;
        let res = libc::readdir(fd, ptr, 128);
        if res == 128 {
            Ok(true)
        } else if res == 0 {
            Ok(false)
        } else {
            Err(FsError::Unknown)
        }
    }

    fn close_dir(&self, fd: i32) {
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
