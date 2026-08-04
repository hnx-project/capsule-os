use super::{FileSystem, FsError};

extern crate libstd;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn unlink(&self, path: &str) -> Result<(), FsError> {
        match libstd::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(_) => Err(FsError::FileNotFound),
        }
    }

    fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        match libstd::fs::remove_dir(path) {
            Ok(()) => Ok(()),
            Err(_) => Err(FsError::PermissionDenied),
        }
    }

    fn is_dir(&self, _path: &str) -> bool {
        false
    }

    fn read_dir_names<F>(&self, _path: &str, _f: F) -> Result<(), FsError>
    where
        F: FnMut(&str),
    {
        Err(FsError::Unknown)
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
