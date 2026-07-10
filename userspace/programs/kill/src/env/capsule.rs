use super::{KillError, ProcSystem};

extern crate hnxlibc;

pub struct CapsuleEnv;

impl ProcSystem for CapsuleEnv {
    fn kill(&self, pid: u32, signal: i32) -> Result<(), KillError> {
        // CapsuleOS does not yet expose a dedicated SYSCALL_KILL — the
        // IPC-bus-less prototype has no signal routing.  We surface a
        // `PermissionDenied` for unknown pids so the calling shell can
        // keep its error-handling path exercised until kill lands.
        let _ = (pid, signal);
        Err(KillError::PermissionDenied)
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = hnxlibc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = hnxlibc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        hnxlibc::exit(code);
    }
}
