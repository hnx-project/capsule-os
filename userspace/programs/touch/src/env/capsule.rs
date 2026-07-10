use super::{FileSystem, FsError};

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn touch(&self, _path: &str, _no_create: bool) -> Result<(), FsError> {
        // TODO: 对接 hnxlibc::open(_path, O_CREAT | O_WRONLY)
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
