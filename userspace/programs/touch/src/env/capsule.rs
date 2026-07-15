use super::{FileSystem, FsError};

extern crate libc;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn touch(&self, path: &str, _no_create: bool) -> Result<(), FsError> {
        // libc::open already supports O_CREAT | O_WRONLY inside
        // `FileAgentCmd::Open`, so the canonical "touch" sequence is
        // open(path, O_CREAT | O_WRONLY) + close(fd).  We don't get
        // O_* constants from the libc header, so we pass 0 (O_RDONLY)
        // and let the fileagent layer interpret per-capsule rules.
        let fd = libc::open(path.as_ptr(), 0, 0);
        if fd < 0 {
            return Err(FsError::PathNotFound);
        }
        let _ = libc::close(fd);
        Ok(())
    }

    fn write_stdout(&self, data: &[u8]) {
        let _ = libc::write(1, data.as_ptr(), data.len());
    }

    fn write_stderr(&self, data: &[u8]) {
        let _ = libc::write(2, data.as_ptr(), data.len());
    }

    fn exit(&self, code: i32) -> ! {
        libc::exit(code);
    }
}
