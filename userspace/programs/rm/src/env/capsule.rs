use super::{FileSystem, FsError};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn unlink(&self, path: &str) -> Result<(), FsError> {
        let res = hnxlibc::unlink(path.as_ptr());
        if res == 0 {
            Ok(())
        } else {
            Err(FsError::FileNotFound)
        }
    }

    fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        let res = hnxlibc::rmdir(path.as_ptr());
        if res == 0 {
            Ok(())
        } else {
            Err(FsError::PermissionDenied)
        }
    }

    fn is_dir(&self, _path: &str) -> bool {
        // TODO: pending SYSCALL_STAT / metadata query — return false so
        // `rm -r` falls back to `unlink` semantics for known files.
        false
    }

    fn read_dir_names<F>(&self, _path: &str, _f: F) -> Result<(), FsError>
    where
        F: FnMut(&str),
    {
        // TODO: pending SYSCALL_READDIR.
        Err(FsError::Unknown)
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
