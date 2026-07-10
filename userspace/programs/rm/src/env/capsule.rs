use super::{FileSystem, FsError};

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn unlink(&self, _path: &str) -> Result<(), FsError> {
        // TODO: 对接 hnxlibc::unlink(_path)
        Ok(())
    }

    fn remove_dir(&self, _path: &str) -> Result<(), FsError> {
        // TODO: 对接 hnxlibc::rmdir(_path)
        Ok(())
    }

    fn is_dir(&self, _path: &str) -> bool {
        // TODO: 底层 metadata query
        false
    }

    fn read_dir_names<F>(&self, _path: &str, mut _f: F) -> Result<(), FsError>
    where
        F: FnMut(&str),
    {
        // TODO: 流式获取子文件/子目录绝对路径列表
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
