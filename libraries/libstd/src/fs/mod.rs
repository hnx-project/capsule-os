use libc;

pub struct File {
    pub fd: i32,
}

impl File {
    pub fn open(path: &str) -> Result<Self, ()> {
        let fd = libc::open_str(path, 0, 0);
        if fd >= 0 {
            Ok(Self { fd })
        } else {
            Err(())
        }
    }

    pub fn create(path: &str) -> Result<Self, ()> {
        // Simple write-create placeholder using 0x41 (O_CREAT | O_WRONLY) flags
        let fd = libc::open_str(path, 0x41, 0);
        if fd >= 0 {
            Ok(Self { fd })
        } else {
            Err(())
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, ()> {
        let n = libc::read(self.fd, buf.as_mut_ptr(), buf.len());
        if n >= 0 {
            Ok(n as usize)
        } else {
            Err(())
        }
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, ()> {
        let n = libc::write(self.fd, buf.as_ptr(), buf.len());
        if n >= 0 {
            Ok(n as usize)
        } else {
            Err(())
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = libc::close(self.fd);
    }
}

pub fn read(_path: &str) -> Result<crate::vec::Vec<u8>, ()> {
    panic!("CapsuleOS: std::fs::read() is not implemented yet");
}

pub fn write(_path: &str, _contents: &[u8]) -> Result<(), ()> {
    panic!("CapsuleOS: std::fs::write() is not implemented yet");
}

pub fn create_dir(_path: &str) -> Result<(), ()> {
    panic!("CapsuleOS: std::fs::create_dir() is not implemented yet");
}

pub fn remove_file(_path: &str) -> Result<(), ()> {
    panic!("CapsuleOS: std::fs::remove_file() is not implemented yet");
}

pub fn remove_dir(_path: &str) -> Result<(), ()> {
    panic!("CapsuleOS: std::fs::remove_dir() is not implemented yet");
}
