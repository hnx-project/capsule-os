use super::{FileSystem, FsError};

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open(&self, _path: &str) -> Result<i32, FsError> {
        // TODO: 对接 hnxlibc::open(_path, 0, 0)
        // 临时桩
        Ok(-1)
    }

    fn read(&self, _fd: i32, _buf: &mut [u8]) -> Result<usize, FsError> {
        // TODO: 对接 hnxlibc::read(_fd, _buf)
        Ok(0)
    }

    fn close(&self, _fd: i32) {
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
