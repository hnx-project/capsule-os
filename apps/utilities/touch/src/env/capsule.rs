use super::{FileSystem, FsError};

extern crate libstd;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn touch(&self, path: &str, _no_create: bool) -> Result<(), FsError> {
        match libstd::fs::File::create(path) {
            Ok(_) => Ok(()),
            Err(_) => Err(FsError::PathNotFound),
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
