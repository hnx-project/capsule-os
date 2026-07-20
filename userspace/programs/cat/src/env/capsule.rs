use super::{FileSystem, FsError};

extern crate libstd;

pub struct CapsuleEnv;

impl FileSystem for CapsuleEnv {
    fn open(&self, path: &str) -> Result<i32, FsError> {
        // Because of the strict Sandbox Purge of programs, we do not expose raw libc to userspace programs.
        // Instead, we can use the safe libstd::fs::File internally.
        // Since File has standard raw implementation inside std::fs::File, but the trait requires returning i32,
        // we can utilize the standard File object.
        // Actually, we can implement open using standard File!
        // But how do we return an i32 fd?
        // Let's look at libraries/libstd/src/fs/mod.rs. The File has a pub fd field.
        // Thus we can safely use libstd::fs::File::open(path) and access .fd!
        match libstd::fs::File::open(path) {
            Ok(file) => {
                // To prevent the File from being dropped (which closes the fd),
                // we can return the raw fd and safely leak it, or keep it.
                // Since this is EL0, leaking an i32 fd for cat is perfectly fine.
                let fd = file.fd;
                core::mem::forget(file);
                Ok(fd)
            }
            Err(_) => Err(FsError::FileNotFound),
        }
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> Result<usize, FsError> {
        // Reconstruct the File from fd without triggering drop unless we want to,
        // or just read from it.
        // Since File has a standard implementation using libc::read, we can also reconstruct it:
        let file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(fd) };
        let res = file.read(buf);
        core::mem::forget(file); // Don't drop / close the fd yet
        match res {
            Ok(n) => Ok(n),
            Err(_) => Err(FsError::Unknown),
        }
    }

    fn close(&self, fd: i32) {
        // Drop it safely to close the file session
        let _file = unsafe { core::mem::transmute::<i32, libstd::fs::File>(fd) };
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
