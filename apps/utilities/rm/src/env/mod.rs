#[derive(Debug)]
pub enum FsError {
    FileNotFound,
    PermissionDenied,
    Unknown,
}

pub trait FileSystem {
    fn unlink(&self, path: &str) -> Result<(), FsError>;
    fn remove_dir(&self, path: &str) -> Result<(), FsError>;
    fn is_dir(&self, path: &str) -> bool;
    fn read_dir_names<F>(&self, path: &str, f: F) -> Result<(), FsError>
    where
        F: FnMut(&str);
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
