#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod cat;
pub mod env;
pub mod options;

use crate::env::FileSystem;
use crate::options::CatOptions;

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();

    let mut opts = CatOptions::new();
    let mut file_path = "";
    let mut file_specified = false;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'n' => opts.number = true,
                    _ => {
                        env.write_stderr(b"Unknown option: -");
                        let mut char_buf = [0u8; 4];
                        let s = c.encode_utf8(&mut char_buf);
                        env.write_stderr(s.as_bytes());
                        env.write_stderr(b"\n");
                        env.exit(1);
                    }
                }
            }
        } else {
            file_path = arg;
            file_specified = true;
        }
    }

    if !file_specified {
        env.write_stderr(b"Usage: cat [-n] <file_path>\n");
        env.exit(1);
    }

    match cat::run_cat(&env, file_path, &opts) {
        Ok(_) => {}
        Err(env::FsError::FileNotFound) => {
            env.write_stderr(b"Error: File not found: ");
            env.write_stderr(file_path.as_bytes());
            env.write_stderr(b"\n");
            env.exit(1);
        }
        Err(env::FsError::PermissionDenied) => {
            env.write_stderr(b"Error: Permission denied\n");
            env.exit(1);
        }
        Err(_) => {
            env.write_stderr(b"Error: Unknown FS Error\n");
            env.exit(1);
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 userspace/hnxlibc/src/lib.rs）。
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。argv 由 kernel
//    透传过来 (见 SYSCALL_EXECVE)，通过 hnxlibc 暴露的 `hnx_argc` /
//    `hnx_arg` 读取。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    use hnxlibc::{hnx_arg, hnx_argc};
    let env = env::capsule::CapsuleEnv;
    let argc = hnx_argc();
    let mut opts = CatOptions::new();
    let mut file_path: &[u8] = &[];
    for i in 1..argc as usize {
        let a = hnx_arg(i);
        if a.first() == Some(&b'-') {
            for &c in &a[1..] {
                if c == b'n' {
                    opts.number = true;
                } else {
                    env.write_stderr(b"Unknown option: -");
                    env.write_stderr(&[c]);
                    env.write_stderr(b"\n");
                    return 1;
                }
            }
        } else if file_path.is_empty() {
            file_path = a;
        }
    }
    if file_path.is_empty() {
        env.write_stderr(b"Usage: cat [-n] <file_path>\n");
        return 1;
    }
    let path_str = match core::str::from_utf8(file_path) {
        Ok(s) => s,
        Err(_) => {
            env.write_stderr(b"Error: Invalid UTF-8 in argv\n");
            return 1;
        }
    };
    match cat::run_cat(&env, path_str, &opts) {
        Ok(_) => 0,
        Err(env::FsError::FileNotFound) => {
            env.write_stderr(b"Error: File not found: ");
            env.write_stderr(path_str.as_bytes());
            env.write_stderr(b"\n");
            1
        }
        Err(env::FsError::PermissionDenied) => {
            env.write_stderr(b"Error: Permission denied\n");
            1
        }
        Err(_) => {
            env.write_stderr(b"Error: Unknown FS Error\n");
            1
        }
    }
}
