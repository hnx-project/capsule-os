use super::{FileSystem, FsError};

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn mkdir(&self, _path: &str) -> Result<(), FsError> {
        // TODO: 对接 hnxlibc::mkdir(_path)
        Ok(())
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
