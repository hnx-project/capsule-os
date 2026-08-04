use super::{Dirent, FileSystem, FsError};
use std::fs::{self, ReadDir};
use std::io::Write;
use std::sync::Mutex;

// Host 端我们用一个静态列表模拟目录描述符（FD），因为标准库 read_dir 是迭代器而非 FD
static DIRECTORIES: Mutex<Vec<Option<ReadDir>>> = Mutex::new(Vec::new());

pub struct HostEnv;

impl FileSystem for HostEnv {
    fn open_dir(&self, path: &str) -> Result<i32, FsError> {
        match fs::read_dir(path) {
            Ok(read_dir) => {
                let mut dirs = DIRECTORIES.lock().unwrap();
                for i in 0..dirs.len() {
                    if dirs[i].is_none() {
                        dirs[i] = Some(read_dir);
                        return Ok(i as i32);
                    }
                }
                dirs.push(Some(read_dir));
                Ok((dirs.len() - 1) as i32)
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => Err(FsError::DirectoryNotFound),
                std::io::ErrorKind::PermissionDenied => Err(FsError::PermissionDenied),
                _ => Err(FsError::Unknown),
            },
        }
    }

    fn readdir(&self, fd: i32, dirent: &mut Dirent) -> Result<bool, FsError> {
        let mut dirs = DIRECTORIES.lock().unwrap();
        if fd < 0 || fd >= dirs.len() as i32 || dirs[fd as usize].is_none() {
            return Err(FsError::Unknown);
        }

        let read_dir = dirs[fd as usize].as_mut().unwrap();
        match read_dir.next() {
            Some(Ok(entry)) => {
                let name_str = entry.file_name();
                let name_bytes = name_str.to_string_lossy();
                let bytes = name_bytes.as_bytes();

                let len = bytes.len().min(dirent.name.len());
                dirent.name_len = len as u8;
                dirent.name[..len].copy_from_slice(&bytes[..len]);

                dirent.ino = 1; // 本地测试环境写死 inode
                if let Ok(meta) = entry.metadata() {
                    dirent.size = meta.len(); // 填充大小
                    if meta.is_dir() {
                        dirent.ftype = 2; // Directory
                    } else {
                        dirent.ftype = 1; // RegularFile
                    }
                } else {
                    dirent.size = 0;
                    dirent.ftype = 0; // Unknown
                }
                Ok(true)
            }
            Some(Err(_)) => Err(FsError::Unknown),
            None => Ok(false), // 读完了
        }
    }

    fn close_dir(&self, fd: i32) {
        let mut dirs = DIRECTORIES.lock().unwrap();
        if fd >= 0 && fd < dirs.len() as i32 {
            dirs[fd as usize] = None;
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
