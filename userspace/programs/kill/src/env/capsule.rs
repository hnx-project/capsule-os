use super::{KillError, ProcSystem};

extern crate libc;
extern crate libstd;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn kill(&self, pid: u32, signal: i32) -> Result<(), KillError> {
        match libc::syscalls::kill(pid as i64, signal as usize) {
            Ok(()) => Ok(()),
            Err(libc::Status::NotFound) | Err(libc::Status::ProcessNotFound) => Err(KillError::ProcessNotFound),
            Err(libc::Status::AccessDenied) => Err(KillError::PermissionDenied),
            Err(_) => Err(KillError::Unknown),
        }
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

    fn exit(&self, code: i32) -> ! {
        libc::exit(code);
    }
}
