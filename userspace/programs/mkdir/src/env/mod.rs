#[derive(Debug)]
pub enum FsError {
    AlreadyExists,
    PathNotFound,
    PermissionDenied,
    Unknown,
}

pub trait FileSystem {
    fn mkdir(&self, path: &str) -> Result<(), FsError>;
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
