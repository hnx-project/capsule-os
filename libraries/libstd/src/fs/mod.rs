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

fn call_with_nul_path<F>(path: &str, f: F) -> Result<(), ()>
where
    F: FnOnce(*const u8) -> i32,
{
    let mut buf = [0u8; 128];
    let len = path.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&path.as_bytes()[..len]);
    buf[len] = 0;
    if f(buf.as_ptr()) == 0 {
        Ok(())
    } else {
        Err(())
    }
}

pub fn read(path: &str) -> Result<crate::vec::Vec<u8>, ()> {
    let file = File::open(path)?;
    let mut v = crate::vec::Vec::new();
    let mut buf = [0u8; 256];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for i in 0..n {
                    if v.push(buf[i]).is_err() {
                        return Err(());
                    }
                }
            }
            _ => return Err(()),
        }
    }
    Ok(v)
}

pub fn write(path: &str, contents: &[u8]) -> Result<(), ()> {
    let file = File::create(path)?;
    let mut offset = 0;
    while offset < contents.len() {
        match file.write(&contents[offset..]) {
            Ok(0) => break,
            Ok(n) => offset += n,
            _ => return Err(()),
        }
    }
    Ok(())
}

pub fn create_dir(path: &str) -> Result<(), ()> {
    call_with_nul_path(path, |p| libc::mkdir(p))
}

pub fn remove_file(path: &str) -> Result<(), ()> {
    call_with_nul_path(path, |p| libc::unlink(p))
}

pub fn remove_dir(path: &str) -> Result<(), ()> {
    call_with_nul_path(path, |p| libc::rmdir(p))
}
