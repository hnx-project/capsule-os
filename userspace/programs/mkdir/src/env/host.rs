use super::{FileSystem, FsError};
use std::fs;
use std::io::Write;

pub struct HostEnv;

impl FileSystem for HostEnv {
    fn mkdir(&self, path: &str) -> Result<(), FsError> {
        match fs::create_dir(path) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                std::io::ErrorKind::AlreadyExists => Err(FsError::AlreadyExists),
                std::io::ErrorKind::NotFound => Err(FsError::PathNotFound),
                std::io::ErrorKind::PermissionDenied => Err(FsError::PermissionDenied),
                _ => Err(FsError::Unknown),
            },
        }
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
