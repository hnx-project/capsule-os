use super::{FileSystem, FsError};

extern crate libstd;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn rmdir(&self, path: &str) -> Result<(), FsError> {
        match libstd::fs::remove_dir(path) {
            Ok(()) => Ok(()),
            Err(_) => Err(FsError::DirectoryNotFound),
        }
    }

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
