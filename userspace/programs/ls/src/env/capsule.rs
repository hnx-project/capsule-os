use super::{Dirent, FileSystem, FsError};

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open_dir(&self, _path: &str) -> Result<i32, FsError> {
        // TODO: 对接 hnxlibc::opendir(_path) 或 open(_path, O_DIRECTORY, 0)
        Ok(-1)
    }

    fn readdir(&self, _fd: i32, _dirent: &mut Dirent) -> Result<bool, FsError> {
        // TODO: 对接 hnxlibc::readdir(_fd, _dirent)
        Ok(false)
    }

    fn close_dir(&self, _fd: i32) {
        // TODO: 对接 hnxlibc::close(_fd)
    }

    fn write_stdout(&self, _data: &[u8]) {
        // TODO: 对接 hnxlibc::write(1, _data)
    }

    fn write_stderr(&self, _data: &[u8]) {
        // TODO: 对接 hnxlibc::write(2, _data)
    }

    fn exit(&self, _code: i32) -> ! {
        // TODO: 对接 hnxlibc::exit(_code)
        loop {}
    }
}
