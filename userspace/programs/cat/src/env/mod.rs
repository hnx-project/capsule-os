#[derive(Debug)]
pub enum FsError {
    FileNotFound,
    PermissionDenied,
    Unknown,
}

pub trait FileSystem {
    fn open(&self, path: &str) -> Result<i32, FsError>;
    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, FsError>;
    fn close(&self, fd: i32);
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
