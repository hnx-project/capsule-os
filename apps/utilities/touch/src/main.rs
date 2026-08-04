#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::FileSystem;

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();

    let mut no_create = false;
    let mut targets = [""; 16];
    let mut target_count = 0;

    for arg in args.iter().skip(1) {
        if arg.starts_with('-') {
            for c in arg.chars().skip(1) {
                match c {
                    'c' => no_create = true,
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
        } else if target_count < 16 {
            targets[target_count] = arg;
            target_count += 1;
        }
    }

    if target_count == 0 {
        env.write_stderr(b"Usage: touch [-c] <file_path>\n");
        env.exit(1);
    }

    for i in 0..target_count {
        let target = targets[i];
        match env.touch(target, no_create) {
            Ok(_) => {}
            Err(env::FsError::PathNotFound) => {
                env.write_stderr(b"Error: Parent path not found: ");
                env.write_stderr(target.as_bytes());
                env.write_stderr(b"\n");
                env.exit(1);
            }
            Err(env::FsError::PermissionDenied) => {
                env.write_stderr(b"Error: Permission denied\n");
                env.exit(1);
            }
            Err(_) => {
                env.write_stderr(b"Error: Unknown Error creating file\n");
                env.exit(1);
            }
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 hnxlibc/src/lib.rs）。
//    我们不再自己实现 `_start` 和 `panic_handler`：这两者都由 hnxlibc
//    统一接管。argv 由 kernel 透传过来 (见 SYSCALL_EXECVE)，通过
//    hnxlibc 暴露的 `hnx_argc` / `hnx_arg` 读取。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    let mut no_create = false;
    let mut targets_arr: [libstd::string::String; 16] = [
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
        libstd::string::String::new(),
    ];
    let mut target_count = 0;

    let mut count = 0;
    for arg in libstd::env::args() {
        if count == 0 {
            count += 1;
            continue;
        }
        let a = arg.as_bytes();
        if a.first() == Some(&b'-') {
            for &c in &a[1..] {
                if c == b'c' {
                    no_create = true;
                } else {
                    env.write_stderr(b"Unknown option: -");
                    env.write_stderr(&[c]);
                    env.write_stderr(b"\n");
                    return 1;
                }
            }
        } else if target_count < 16 {
            targets_arr[target_count] = arg;
            target_count += 1;
        }
        count += 1;
    }

    if target_count == 0 {
        env.write_stderr(b"Usage: touch [-c] <file_path>\n");
        return 1;
    }

    let mut rc = 0;
    for i in 0..target_count {
        let path = targets_arr[i].as_str();
        match env.touch(path, no_create) {
            Ok(_) => {}
            Err(env::FsError::PathNotFound) => {
                env.write_stderr(b"Error: Parent path not found: ");
                env.write_stderr(path.as_bytes());
                env.write_stderr(b"\n");
                rc = 1;
            }
            Err(env::FsError::PermissionDenied) => {
                env.write_stderr(b"Error: Permission denied\n");
                rc = 1;
            }
            Err(_) => {
                env.write_stderr(b"Error: Unknown Error creating file\n");
                rc = 1;
            }
        }
    }
    rc
}
