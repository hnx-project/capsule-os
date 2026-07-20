#[derive(Debug)]
pub enum FsError {
    PathNotFound,
    PermissionDenied,
    Unknown,
}

pub trait FileSystem {
    fn touch(&self, path: &str, no_create: bool) -> Result<(), FsError>;
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
