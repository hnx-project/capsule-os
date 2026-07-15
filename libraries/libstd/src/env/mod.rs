use libc;

pub struct Args {
    index: usize,
    count: usize,
}

impl Iterator for Args {
    type Item = crate::string::String;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let arg_bytes = libc::hnx_arg(self.index);
        self.index += 1;
        if arg_bytes.is_empty() {
            return None;
        }
        // Convert static u8 slice to owned String safely
        let mut s = crate::string::String::new();
        for &b in arg_bytes {
            let _ = s.push_byte(b);
        }
        Some(s)
    }
}

pub fn args() -> Args {
    let argc = libc::hnx_argc() as usize;
    Args {
        index: 0,
        count: argc,
    }
}

pub fn current_dir() -> Result<crate::string::String, ()> {
    let mut buf = [0u8; 256];
    match libc::getcwd(&mut buf) {
        Ok(len) => {
            let mut s = crate::string::String::new();
            for &b in &buf[..len] {
                if b == 0 {
                    break;
                }
                let _ = s.push_byte(b);
            }
            Ok(s)
        }
        Err(_) => Err(()),
    }
}

pub fn set_current_dir(path: &str) -> Result<(), ()> {
    match libc::chdir(path) {
        Ok(()) => Ok(()),
        Err(_) => Err(()),
    }
}

pub fn var(_key: &str) -> Result<crate::string::String, ()> {
    panic!("CapsuleOS: std::env::var() is not implemented yet");
}

pub fn set_var(_key: &str, _value: &str) {
    panic!("CapsuleOS: std::env::set_var() is not implemented yet");
}
