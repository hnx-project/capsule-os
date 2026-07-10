use super::{FileSystem, FsError};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn rmdir(&self, path: &str) -> Result<(), FsError> {
        let res = hnxlibc::rmdir(path.as_ptr());
        if res == 0 {
            Ok(())
        } else {
            Err(FsError::DirectoryNotFound)
        }
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
