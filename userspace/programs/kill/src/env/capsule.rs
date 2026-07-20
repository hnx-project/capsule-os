use super::{KillError, ProcSystem};

extern crate libstd;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn kill(&self, _pid: u32, _signal: i32) -> Result<(), KillError> {
        Err(KillError::PermissionDenied)
    }

    fn write_stdout(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn write_stderr(&self, data: &[u8]) {
        if let Ok(s) = core::str::from_utf8(data) {
            libstd::io::print(s);
        }
    }

    fn exit(&self, _code: i32) -> ! {
        panic!("Process exited");
    }
}
