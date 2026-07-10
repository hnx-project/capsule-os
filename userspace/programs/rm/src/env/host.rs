use super::{FileSystem, FsError};
use std::fs;
use std::io::Write;

pub struct HostEnv;

impl FileSystem for HostEnv {
    fn unlink(&self, path: &str) -> Result<(), FsError> {
        match fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(FsError::FileNotFound),
                std::io::ErrorKind::PermissionDenied => Err(FsError::PermissionDenied),
                _ => Err(FsError::Unknown),
            },
        }
    }

    fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        match fs::remove_dir(path) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(FsError::FileNotFound),
                std::io::ErrorKind::PermissionDenied => Err(FsError::PermissionDenied),
                _ => Err(FsError::Unknown),
            },
        }
    }

    fn is_dir(&self, path: &str) -> bool {
        match fs::metadata(path) {
            Ok(meta) => meta.is_dir(),
            Err(_) => false,
        }
    }

    fn read_dir_names<F>(&self, path: &str, mut f: F) -> Result<(), FsError>
    where
        F: FnMut(&str),
    {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path_buf = entry.path();
                        if let Some(path_str) = path_buf.to_str() {
                            f(path_str);
                        }
                    }
                }
                Ok(())
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(FsError::FileNotFound),
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
