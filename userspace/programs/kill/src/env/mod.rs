#[derive(Debug)]
pub enum KillError {
    ProcessNotFound,
    PermissionDenied,
    Unknown,
}

pub trait ProcSystem {
    fn kill(&self, pid: u32, signal: i32) -> Result<(), KillError>;
    fn write_stdout(&self, data: &[u8]);
    fn write_stderr(&self, data: &[u8]);
    fn exit(&self, code: i32) -> !;
}

#[cfg(feature = "host")]
pub mod host;

#[cfg(not(feature = "host"))]
pub mod capsule;
