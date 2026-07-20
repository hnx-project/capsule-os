use super::{FileSystem, FsError};
use std::fs::OpenOptions;
use std::io::Write;

pub struct HostEnv;

impl FileSystem for HostEnv {
    fn touch(&self, path: &str, no_create: bool) -> Result<(), FsError> {
        // 如果 no_create 为 true 且文件不存在，则直接成功退出而不创建
        if no_create && !std::path::Path::new(path).exists() {
            return Ok(());
        }

        match OpenOptions::new().write(true).create(!no_create).open(path) {
            Ok(_) => Ok(()),
            Err(e) => match e.kind() {
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
