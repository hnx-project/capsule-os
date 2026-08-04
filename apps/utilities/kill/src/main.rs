#![cfg_attr(not(feature = "host"), no_std)]
#![cfg_attr(not(feature = "host"), no_main)]

pub mod env;

use crate::env::ProcSystem;

/// 解析极简字符串为数字而无需标准库支持 (no_std)
#[allow(dead_code)]
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0u32;
    for c in s.chars() {
        if c.is_ascii_digit() {
            val = val * 10 + (c as u32 - '0' as u32);
        } else {
            return None;
        }
    }
    Some(val)
}

// 2. 本地测试环境入口 (使用标准库)
#[cfg(feature = "host")]
fn main() {
    let env = env::host::HostEnv;
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        env.write_stderr(b"Usage: kill <pid>\n");
        env.exit(1);
    }

    let pid_str = &args[1];
    match parse_u32(pid_str) {
        Some(pid) => {
            match env.kill(pid, 15) {
                // 15 = SIGTERM
                Ok(_) => {
                    env.write_stdout(b"Process ");
                    env.write_stdout(pid_str.as_bytes());
                    env.write_stdout(b" terminated.\n");
                }
                Err(env::KillError::ProcessNotFound) => {
                    env.write_stderr(b"Error: Process not found: ");
                    env.write_stderr(pid_str.as_bytes());
                    env.write_stderr(b"\n");
                    env.exit(1);
                }
                Err(env::KillError::PermissionDenied) => {
                    env.write_stderr(b"Error: Permission denied\n");
                    env.exit(1);
                }
                Err(_) => {
                    env.write_stderr(b"Error: Unknown kill error\n");
                    env.exit(1);
                }
            }
        }
        None => {
            env.write_stderr(b"Error: Invalid PID: ");
            env.write_stderr(pid_str.as_bytes());
            env.write_stderr(b"\n");
            env.exit(1);
        }
    }
}

// 3. Capsule OS 入口：hnxlibc 在 _start 之后会调用
//    `extern "Rust" { fn main() -> i32 }`（见 hnxlibc/src/lib.rs）。
//    `_start` 和 `panic_handler` 都由 hnxlibc 统一接管。
#[cfg(not(feature = "host"))]
#[no_mangle]
pub fn main() -> i32 {
    let env = env::capsule::CapsuleEnv;
    let mut pid_str = libstd::string::String::new();
    
    let mut count = 0;
    for arg in libstd::env::args() {
        if count == 1 {
            pid_str = arg;
        }
        count += 1;
    }

    if count < 2 || pid_str.len() == 0 {
        env.write_stderr(b"Usage: kill <pid>\n");
        return 1;
    }

    match parse_u32(pid_str.as_str()) {
        Some(pid) => {
            match env.kill(pid, 15) {
                // 15 = SIGTERM
                Ok(_) => {
                    env.write_stdout(b"Process ");
                    env.write_stdout(pid_str.as_bytes());
                    env.write_stdout(b" terminated.\n");
                    0
                }
                Err(env::KillError::ProcessNotFound) => {
                    env.write_stderr(b"Error: Process not found: ");
                    env.write_stderr(pid_str.as_bytes());
                    env.write_stderr(b"\n");
                    1
                }
                Err(env::KillError::PermissionDenied) => {
                    env.write_stderr(b"Error: Permission denied\n");
                    1
                }
                Err(_) => {
                    env.write_stderr(b"Error: Unknown kill error\n");
                    1
                }
            }
        }
        None => {
            env.write_stderr(b"Error: Invalid PID: ");
            env.write_stderr(pid_str.as_bytes());
            env.write_stderr(b"\n");
            1
        }
    }
}
