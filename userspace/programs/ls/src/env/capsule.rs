use super::{Dirent, FileSystem, FsError};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open_dir(&self, _path: &str) -> Result<i32, FsError> {
        // TODO: SYSCALL_READDIR + FileAgent ReadDir 协议尚未落地
        // （参见 capsule-design.md §2）。  直接返 DirectoryNotFound 让
        // ls 在 shell 里走"未实现"路径。
        Err(FsError::DirectoryNotFound)
    }

    fn readdir(&self, _fd: i32, _dirent: &mut Dirent) -> Result<bool, FsError> {
        Ok(false)
    }

    fn close_dir(&self, _fd: i32) {}

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
