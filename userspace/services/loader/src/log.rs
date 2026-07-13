use hnxlibc;
use shared::status::Status;

pub struct Logger;

impl Logger {
    /// Write a direct message to standard output safely by staging on the warm stack
    pub fn write(msg: &str) {
        let mut stack_buf = [0u8; 128];
        let len = core::cmp::min(msg.len(), 127);
        stack_buf[..len].copy_from_slice(&msg.as_bytes()[..len]);
        let _ = hnxlibc::write(1, stack_buf.as_ptr(), len);
    }

    /// Print status for a spawned process
    pub fn print_spawn_status(name: &str, result: Result<u64, Status>) {
        let mut buf = [0u8; 128];
        let mut cursor = 0;

        let prefix = b"Loader: spawned ";
        buf[cursor..cursor + prefix.len()].copy_from_slice(prefix);
        cursor += prefix.len();

        let name_bytes = name.as_bytes();
        buf[cursor..cursor + name_bytes.len()].copy_from_slice(name_bytes);
        cursor += name_bytes.len();

        match result {
            Ok(pid) => {
                let infix = b" (pid=";
                buf[cursor..cursor + infix.len()].copy_from_slice(infix);
                cursor += infix.len();

                let mut temp = [0u8; 20];
                let mut temp_len = 0;
                let mut n = pid;
                if n == 0 {
                    temp[0] = b'0';
                    temp_len = 1;
                } else {
                    while n > 0 {
                        temp[temp_len] = b'0' + (n % 10) as u8;
                        n /= 10;
                        temp_len += 1;
                    }
                }
                for j in 0..temp_len {
                    buf[cursor + j] = temp[temp_len - 1 - j];
                }
                cursor += temp_len;

                let suffix = b")\n";
                buf[cursor..cursor + suffix.len()].copy_from_slice(suffix);
                cursor += suffix.len();
            }
            Err(err) => {
                let infix = b" failed with status ";
                buf[cursor..cursor + infix.len()].copy_from_slice(infix);
                cursor += infix.len();

                let code = err.to_raw() as isize as i32;
                let mut temp = [0u8; 20];
                let mut temp_len = 0;
                let is_neg = code < 0;
                let mut n = if is_neg { (-code) as u64 } else { code as u64 };
                if n == 0 {
                    temp[0] = b'0';
                    temp_len = 1;
                } else {
                    while n > 0 {
                        temp[temp_len] = b'0' + (n % 10) as u8;
                        n /= 10;
                        temp_len += 1;
                    }
                    if is_neg {
                        temp[temp_len] = b'-';
                        temp_len += 1;
                    }
                }
                for j in 0..temp_len {
                    buf[cursor + j] = temp[temp_len - 1 - j];
                }
                cursor += temp_len;

                buf[cursor] = b'\n';
                cursor += 1;
            }
        }

        if let Ok(s) = core::str::from_utf8(&buf[..cursor]) {
            Self::write(s);
        }
    }
}
