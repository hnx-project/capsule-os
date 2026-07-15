use super::{Dirent, FileSystem, FsError};

extern crate libstd;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open_dir(&self, _path: &str) -> Result<i32, FsError> {
        Err(FsError::DirectoryNotFound)
    }

    fn readdir(&self, _fd: i32, _dirent: &mut Dirent) -> Result<bool, FsError> {
        Ok(false)
    }

    fn close_dir(&self, _fd: i32) {}

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

    fn exit(&self, _code: i32) -> ! {
        panic!("Process exited");
    }
}
